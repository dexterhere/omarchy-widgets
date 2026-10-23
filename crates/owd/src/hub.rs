//! Provider registry, RAM cache, subscriptions and the polling scheduler.
//!
//! Sensitive data lives only in this process's memory. Subscriptions exist only while a
//! client holds them (the bridge unsubscribes hidden widgets), so providers are polled only
//! while someone is looking. Grants are re-checked on every push and after every fetch.
//!
//! Fetches run on short-lived worker threads (at most `MAX_IN_FLIGHT`), at most one per key,
//! so a slow provider delays only its own topic and never the scheduler or shutdown.
use crate::conn::{Shared, lock};
use ow_core::{FetchCtx, Provider, ProviderError, TopicSpec};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const MAX_SUBS_PER_CONN: usize = 32;
const MAX_SUBS: usize = 256;
const MAX_SUBS_PER_WIDGET: usize = 16;
const MAX_CACHE_KEYS: usize = 256;
const MAX_IN_FLIGHT: usize = 16;
/// Largest provider output we keep or send (serialized). Keeps one push under the 64 KiB
/// line limit clients are held to and bounds queued memory per connection.
const MAX_OUTPUT_BYTES: usize = 48 * 1024;
/// Backoff multiplier cap: interval * 2^4 after repeated failures.
const MAX_BACKOFF_SHIFT: u32 = 4;
/// Unsubscribed cache entries idle longer than this are dropped.
const CACHE_IDLE: Duration = Duration::from_secs(60);

type Params = Map<String, Value>;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    topic: String,
    params: String, // canonical JSON (serde_json maps are sorted)
}

impl Key {
    fn new(topic: &str, params: &Params) -> Key {
        Key {
            topic: topic.to_owned(),
            params: serde_json::to_string(params).unwrap_or_default(),
        }
    }
}

struct Entry {
    data: Option<(Params, u64)>,
    fails: u32,
    next_due: Instant,
    last_touch: Instant,
    /// A worker is fetching this key right now (request coalescing).
    fetching: bool,
}

impl Entry {
    fn snap(&self) -> Option<Snap> {
        self.data.clone().map(|(data, ts)| Snap {
            data,
            ts,
            stale: self.fails > 0,
        })
    }
}

struct Sub {
    conn: u64,
    widget: String,
    key: Key,
    params: Params,
    tx: SyncSender<String>,
    /// Pushes start only after the subscribe reply is queued.
    active: bool,
}

#[derive(Default)]
struct State {
    cache: HashMap<Key, Entry>,
    subs: HashMap<u64, Sub>,
    next_sub: u64,
}

pub(crate) struct Snap {
    pub data: Params,
    pub ts: u64,
    pub stale: bool,
}

pub(crate) struct Hub {
    providers: Vec<Arc<dyn Provider>>,
    state: Mutex<State>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub(crate) fn push_line(sub: u64, topic: &str, snap: &Snap, data: &Params) -> String {
    let m = serde_json::json!({
        "v": ow_proto::PROTOCOL, "sub": sub, "topic": topic,
        "ts": snap.ts, "stale": snap.stale, "data": data,
    });
    format!("{m}\n")
}

impl Hub {
    pub(crate) fn new(providers: Vec<Arc<dyn Provider>>) -> Hub {
        Hub {
            providers,
            state: Mutex::new(State::default()),
        }
    }

    fn find(&self, topic: &str) -> Option<(&Arc<dyn Provider>, &'static TopicSpec)> {
        self.providers
            .iter()
            .find_map(|p| p.topics().iter().find(|t| t.name == topic).map(|t| (p, t)))
    }

    /// Claim the right to fetch `key` (creating its entry, evicting an idle one if full).
    /// False when a fetch is already running, the worker cap is reached, or the cache is
    /// full of subscribed entries.
    fn begin(&self, key: &Key) -> bool {
        let now = Instant::now();
        let mut st = lock(&self.state);
        if st.cache.values().filter(|e| e.fetching).count() >= MAX_IN_FLIGHT {
            return false;
        }
        if !st.cache.contains_key(key) && st.cache.len() >= MAX_CACHE_KEYS {
            // Evict the least recently used entry nobody is subscribed to and nobody is
            // fetching, so a flood of distinct keys cannot lock other widgets out.
            let subscribed: HashSet<&Key> = st.subs.values().map(|s| &s.key).collect();
            let victim = st
                .cache
                .iter()
                .filter(|(k, e)| !e.fetching && !subscribed.contains(k))
                .min_by_key(|(_, e)| e.last_touch)
                .map(|(k, _)| k.clone());
            let Some(victim) = victim else { return false };
            st.cache.remove(&victim);
        }
        let e = st.cache.entry(key.clone()).or_insert(Entry {
            data: None,
            fails: 0,
            next_due: now,
            last_touch: now,
            fetching: false,
        });
        e.last_touch = now;
        if e.fetching {
            return false;
        }
        e.fetching = true;
        true
    }

    /// Release a claim without fetching (worker thread could not be started).
    fn abort(&self, key: &Key) {
        if let Some(e) = lock(&self.state).cache.get_mut(key) {
            e.fetching = false;
        }
    }

    /// Run the fetch for a claimed key (no lock held while the provider runs), store the
    /// result and return the best data we have. A panicking provider counts as unavailable;
    /// oversized output counts as a bad format.
    fn complete(&self, key: &Key, params: &Params) -> Option<Snap> {
        let (provider, spec) = self.find(&key.topic)?;
        let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
            provider.fetch(&key.topic, params, &FetchCtx::none())
        }))
        .unwrap_or(Err(ProviderError::Unavailable))
        .map(|d| spec.sanitize(d))
        .and_then(|d| {
            let size = serde_json::to_string(&d).map_or(usize::MAX, |s| s.len());
            if size > MAX_OUTPUT_BYTES {
                Err(ProviderError::Format)
            } else {
                Ok(d)
            }
        });
        let now = Instant::now();
        let mut st = lock(&self.state);
        let Some(e) = st.cache.get_mut(key) else {
            // Evicted while fetching (cannot happen for claimed keys; defensive).
            return res.ok().map(|data| Snap {
                data,
                ts: now_unix(),
                stale: false,
            });
        };
        e.fetching = false;
        e.last_touch = now;
        match res {
            Ok(d) => {
                e.data = Some((d, now_unix()));
                e.fails = 0;
                e.next_due = now + spec.interval;
            }
            Err(_) => {
                e.fails = (e.fails + 1).min(MAX_BACKOFF_SHIFT);
                e.next_due = now + spec.interval * (1 << e.fails);
            }
        }
        e.snap()
    }

    /// Cached value while the entry is not due (marked stale during failure backoff, and no
    /// fetch is attempted); otherwise one coalesced fetch.
    pub(crate) fn snapshot(&self, topic: &str, params: &Params) -> Result<Snap, ProviderError> {
        let (_, spec) = self.find(topic).ok_or(ProviderError::UnknownTopic)?;
        spec.check_params(params)?;
        let key = Key::new(topic, params);
        {
            let mut st = lock(&self.state);
            if let Some(e) = st.cache.get_mut(&key) {
                e.last_touch = Instant::now();
                if e.next_due > e.last_touch || e.fetching {
                    return e.snap().ok_or(ProviderError::Unavailable);
                }
            }
        }
        if self.begin(&key) {
            self.complete(&key, params)
                .ok_or(ProviderError::Unavailable)
        } else {
            lock(&self.state)
                .cache
                .get(&key)
                .and_then(Entry::snap)
                .ok_or(ProviderError::Unavailable)
        }
    }

    /// Whatever is cached right now for an existing subscription; never fetches, so the
    /// connection thread never waits on a provider here.
    pub(crate) fn cached(&self, topic: &str, params: &Params) -> Option<Snap> {
        lock(&self.state)
            .cache
            .get(&Key::new(topic, params))
            .and_then(Entry::snap)
    }

    pub(crate) fn subscribe(
        &self,
        conn: u64,
        widget: &str,
        topic: &str,
        params: Params,
        tx: SyncSender<String>,
    ) -> Result<u64, ow_proto::ErrorCode> {
        use ow_proto::ErrorCode;
        let Some((_, spec)) = self.find(topic) else {
            return Err(ErrorCode::UnknownTopic);
        };
        spec.check_params(&params)
            .map_err(|_| ErrorCode::BadRequest)?;
        let mut st = lock(&self.state);
        let mine = st.subs.values().filter(|s| s.conn == conn).count();
        let widget_subs = st.subs.values().filter(|s| s.widget == widget).count();
        if mine >= MAX_SUBS_PER_CONN
            || widget_subs >= MAX_SUBS_PER_WIDGET
            || st.subs.len() >= MAX_SUBS
        {
            return Err(ErrorCode::RateLimited);
        }
        st.next_sub += 1;
        let id = st.next_sub;
        let key = Key::new(topic, &params);
        st.subs.insert(
            id,
            Sub {
                conn,
                widget: widget.to_owned(),
                key,
                params,
                tx,
                active: false,
            },
        );
        Ok(id)
    }

    /// Start pushes for a subscription; called after its reply is queued.
    pub(crate) fn activate(&self, sub: u64) {
        if let Some(s) = lock(&self.state).subs.get_mut(&sub) {
            s.active = true;
        }
    }

    pub(crate) fn unsubscribe(&self, conn: u64, sub: u64) -> bool {
        let mut st = lock(&self.state);
        if st.subs.get(&sub).is_some_and(|s| s.conn == conn) {
            st.subs.remove(&sub);
            return true;
        }
        false
    }

    pub(crate) fn remove_conn(&self, conn: u64) {
        lock(&self.state).subs.retain(|_, s| s.conn != conn);
    }

    /// Topic, params and widget of a subscription, for the initial push.
    pub(crate) fn sub_target(&self, sub: u64) -> Option<(String, Params, String)> {
        lock(&self.state)
            .subs
            .get(&sub)
            .map(|s| (s.key.topic.clone(), s.params.clone(), s.widget.clone()))
    }

    fn drop_subs(&self, ids: &[u64]) {
        let mut st = lock(&self.state);
        for id in ids {
            st.subs.remove(id);
        }
    }
}

/// One scheduler pass: for every subscribed key that is due, claim it and fetch on a worker
/// thread, then push to its subscribers. Never blocks on a provider.
pub(crate) fn poll_once(shared: &Arc<Shared>) {
    let hub = &shared.hub;
    let now = Instant::now();
    let due: Vec<(Key, Params)> = {
        let mut st = lock(&hub.state);
        let subscribed: HashSet<Key> = st.subs.values().map(|s| s.key.clone()).collect();
        // Idle entries go; subscribed or in-flight ones keep their backoff state.
        st.cache
            .retain(|k, e| e.fetching || subscribed.contains(k) || e.last_touch + CACHE_IDLE > now);
        let mut seen = HashSet::new();
        st.subs
            .values()
            .filter(|s| s.active && seen.insert(s.key.clone()))
            .filter(|s| {
                st.cache
                    .get(&s.key)
                    .is_none_or(|e| !e.fetching && e.next_due <= now)
            })
            .map(|s| (s.key.clone(), s.params.clone()))
            .collect()
    };
    for (key, params) in due {
        if !hub.begin(&key) {
            continue; // already running, worker cap reached, or cache full: try next tick
        }
        let sh = Arc::clone(shared);
        let k2 = key.clone();
        let spawned = std::thread::Builder::new().spawn(move || {
            if let Some(snap) = sh.hub.complete(&k2, &params) {
                push_to_subscribers(&sh, &k2, &snap);
            }
        });
        if spawned.is_err() {
            hub.abort(&key);
        }
    }
}

fn push_to_subscribers(shared: &Shared, key: &Key, snap: &Snap) {
    let targets: Vec<(u64, String)> = lock(&shared.hub.state)
        .subs
        .iter()
        .filter(|(_, s)| s.active && &s.key == key)
        .map(|(id, s)| (*id, s.widget.clone()))
        .collect();
    let mut dead = Vec::new();
    for (id, widget) in targets {
        // Re-check on every push: a revoked grant stops data immediately.
        let grant = shared
            .grants
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .find(&widget, &key.topic)
            .cloned();
        let Some(grant) = grant else {
            lock(&shared.audit).record(&widget, &key.topic, "push", false, 0);
            dead.push(id);
            continue;
        };
        let line = push_line(id, &key.topic, snap, &grant.filter(snap.data.clone()));
        // Send while holding the state lock and re-checking membership: an unsubscribe that
        // already queued its reply cannot be followed by a push, because removal needs the
        // same lock. try_send never blocks.
        let st = lock(&shared.hub.state);
        let Some(sub) = st.subs.get(&id) else {
            continue;
        };
        if matches!(
            sub.tx.try_send(line),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_))
        ) {
            // Slow or gone consumer: drop the subscription rather than buffer without bound.
            dead.push(id);
        }
    }
    shared.hub.drop_subs(&dead);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use ow_core::Sensitivity;
    use ow_proto::ErrorCode;
    use std::sync::mpsc::sync_channel;

    struct P;

    static TOPICS: [TopicSpec; 1] =
        [
            TopicSpec::new("t.one", Sensitivity::Low, Duration::from_secs(1), &["v"])
                .with_params(&["id"]),
        ];

    impl Provider for P {
        fn id(&self) -> &'static str {
            "p"
        }
        fn topics(&self) -> &'static [TopicSpec] {
            &TOPICS
        }
        fn fetch(&self, _t: &str, p: &Params, _c: &FetchCtx<'_>) -> Result<Params, ProviderError> {
            Ok(serde_json::json!({ "v": p.len() })
                .as_object()
                .cloned()
                .unwrap())
        }
    }

    impl Hub {
        fn fetch_now(&self, key: &Key, params: &Params) -> Option<Snap> {
            if self.begin(key) {
                self.complete(key, params)
            } else {
                None
            }
        }
    }

    fn hub() -> Hub {
        Hub::new(vec![Arc::new(P)])
    }

    fn params(id: usize) -> Params {
        serde_json::json!({ "id": id })
            .as_object()
            .cloned()
            .unwrap()
    }

    #[test]
    fn one_widget_cannot_exceed_its_subscription_cap() {
        let h = hub();
        let (tx, _rx) = sync_channel(1);
        for i in 0..MAX_SUBS_PER_WIDGET {
            // distinct connections so the per-connection cap is not what trips
            h.subscribe(i as u64, "w", "t.one", params(i), tx.clone())
                .unwrap();
        }
        let e = h
            .subscribe(99, "w", "t.one", params(99), tx.clone())
            .unwrap_err();
        assert_eq!(e, ErrorCode::RateLimited);
        // another widget is unaffected
        h.subscribe(100, "other", "t.one", params(1), tx).unwrap();
    }

    #[test]
    fn cache_flood_evicts_idle_entries_but_never_subscribed_ones() {
        let h = hub();
        let (tx, _rx) = sync_channel(1);
        h.subscribe(1, "w", "t.one", params(0), tx).unwrap();
        let keep = Key::new("t.one", &params(0));
        h.fetch_now(&keep, &params(0)).unwrap();
        for i in 1..(MAX_CACHE_KEYS * 2) {
            // each distinct key is accepted by evicting an idle one
            assert!(
                h.fetch_now(&Key::new("t.one", &params(i)), &params(i))
                    .is_some(),
                "key {i}"
            );
        }
        let st = lock(&h.state);
        assert!(st.cache.len() <= MAX_CACHE_KEYS);
        assert!(st.cache.contains_key(&keep), "subscribed entry was evicted");
    }

    struct Counting {
        fetches: std::sync::atomic::AtomicUsize,
        delay: Duration,
        fail: std::sync::atomic::AtomicBool,
    }

    impl Provider for Counting {
        fn id(&self) -> &'static str {
            "c"
        }
        fn topics(&self) -> &'static [TopicSpec] {
            &TOPICS
        }
        fn fetch(&self, _t: &str, _p: &Params, _c: &FetchCtx<'_>) -> Result<Params, ProviderError> {
            self.fetches
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(self.delay);
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(ProviderError::Unavailable);
            }
            Ok(serde_json::json!({ "v": 1 }).as_object().cloned().unwrap())
        }
    }

    fn counting(delay: Duration) -> (Arc<Counting>, Hub) {
        let c = Arc::new(Counting {
            fetches: 0.into(),
            delay,
            fail: false.into(),
        });
        (Arc::clone(&c), Hub::new(vec![c]))
    }

    #[test]
    fn concurrent_misses_are_coalesced_into_one_fetch() {
        let (c, h) = counting(Duration::from_millis(200));
        let h = Arc::new(h);
        let ths: Vec<_> = (0..8)
            .map(|_| {
                let h = Arc::clone(&h);
                std::thread::spawn(move || h.snapshot("t.one", &Params::new()))
            })
            .collect();
        let ok = ths
            .into_iter()
            .filter_map(|t| t.join().unwrap().ok())
            .count();
        assert!(ok >= 1);
        assert_eq!(c.fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn failure_backoff_is_honoured_and_stale_is_reported() {
        let (c, h) = counting(Duration::ZERO);
        assert!(!h.snapshot("t.one", &Params::new()).unwrap().stale);
        // expire the entry, then fail once
        lock(&h.state)
            .cache
            .values_mut()
            .for_each(|e| e.next_due = Instant::now());
        c.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        let k = Key::new("t.one", &Params::new());
        let snap = h.fetch_now(&k, &Params::new()).unwrap();
        assert!(snap.stale, "failed refresh with older data must be stale");
        let n = c.fetches.load(std::sync::atomic::Ordering::SeqCst);
        // inside the backoff window further snapshots must not fetch, and stay stale
        let again = h.snapshot("t.one", &Params::new()).unwrap();
        assert!(again.stale);
        assert_eq!(c.fetches.load(std::sync::atomic::Ordering::SeqCst), n);
    }

    #[test]
    fn failure_with_no_data_is_an_error_not_a_refetch_storm() {
        let (c, h) = counting(Duration::ZERO);
        c.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(h.snapshot("t.one", &Params::new()).is_err());
        assert!(h.snapshot("t.one", &Params::new()).is_err());
        assert!(h.snapshot("t.one", &Params::new()).is_err());
        assert_eq!(c.fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn params_are_validated_before_any_fetch() {
        let h = hub();
        let bad = serde_json::json!({ "nope": 1 })
            .as_object()
            .cloned()
            .unwrap();
        assert_eq!(
            h.snapshot("t.one", &bad).err(),
            Some(ProviderError::BadParams)
        );
        let nested = serde_json::json!({ "id": { "a": 1 } })
            .as_object()
            .cloned()
            .unwrap();
        assert_eq!(
            h.snapshot("t.one", &nested).err(),
            Some(ProviderError::BadParams)
        );
        assert!(h.snapshot("t.one", &params(1)).is_ok());
    }
}
