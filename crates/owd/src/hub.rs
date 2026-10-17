//! Provider registry, RAM cache, subscriptions and the polling scheduler.
//!
//! Sensitive data lives only in this process's memory. Subscriptions exist only while a
//! client holds them (the bridge unsubscribes hidden widgets), so providers are polled only
//! while someone is looking. Grants are re-checked on every push.
use crate::conn::{Shared, lock};
use ow_core::{Provider, ProviderError, TopicSpec};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const MAX_SUBS_PER_CONN: usize = 32;
const MAX_SUBS: usize = 256;
const MAX_CACHE_KEYS: usize = 256;
/// Backoff multiplier cap: interval * 2^4 after repeated failures.
const MAX_BACKOFF_SHIFT: u32 = 4;
/// Unsubscribed cache entries older than this are dropped.
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
}

struct Sub {
    conn: u64,
    widget: String,
    key: Key,
    params: Params,
    tx: SyncSender<String>,
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

    pub(crate) fn knows(&self, topic: &str) -> bool {
        self.find(topic).is_some()
    }

    /// Fetch from the provider (no lock held), update the cache, return the best data we have.
    /// A panicking provider counts as unavailable instead of killing the scheduler.
    fn refresh(&self, key: &Key, params: &Params) -> Option<Snap> {
        let (provider, spec) = self.find(&key.topic)?;
        let res = std::panic::catch_unwind(AssertUnwindSafe(|| provider.fetch(&key.topic, params)))
            .unwrap_or(Err(ProviderError::Unavailable))
            .map(|d| spec.sanitize(d));
        let now = Instant::now();
        let mut st = lock(&self.state);
        if !st.cache.contains_key(key) && st.cache.len() >= MAX_CACHE_KEYS {
            return None;
        }
        let e = st.cache.entry(key.clone()).or_insert(Entry {
            data: None,
            fails: 0,
            next_due: now,
            last_touch: now,
        });
        e.last_touch = now;
        let stale = res.is_err();
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
        e.data.clone().map(|(data, ts)| Snap { data, ts, stale })
    }

    /// Cached while fresh, otherwise fetched now.
    pub(crate) fn snapshot(&self, topic: &str, params: &Params) -> Result<Snap, ProviderError> {
        if !self.knows(topic) {
            return Err(ProviderError::UnknownTopic);
        }
        let key = Key::new(topic, params);
        {
            let mut st = lock(&self.state);
            if let Some(e) = st.cache.get_mut(&key) {
                e.last_touch = Instant::now();
                if e.next_due > e.last_touch {
                    if let Some((data, ts)) = e.data.clone() {
                        return Ok(Snap {
                            data,
                            ts,
                            stale: false,
                        });
                    }
                }
            }
        }
        self.refresh(&key, params).ok_or(ProviderError::Unavailable)
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
        if !self.knows(topic) {
            return Err(ErrorCode::UnknownTopic);
        }
        let mut st = lock(&self.state);
        let mine = st.subs.values().filter(|s| s.conn == conn).count();
        if mine >= MAX_SUBS_PER_CONN || st.subs.len() >= MAX_SUBS {
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
            },
        );
        Ok(id)
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

    /// Current subscription as (topic, params), for the initial push.
    pub(crate) fn sub_target(&self, sub: u64) -> Option<(String, Params)> {
        lock(&self.state)
            .subs
            .get(&sub)
            .map(|s| (s.key.topic.clone(), s.params.clone()))
    }

    pub(crate) fn widget_of(&self, sub: u64) -> Option<String> {
        lock(&self.state).subs.get(&sub).map(|s| s.widget.clone())
    }

    fn drop_subs(&self, ids: &[u64]) {
        let mut st = lock(&self.state);
        for id in ids {
            st.subs.remove(id);
        }
    }
}

/// One scheduler pass: refresh every subscribed key that is due and push to its subscribers.
pub(crate) fn poll_once(shared: &Shared) {
    let hub = &shared.hub;
    let now = Instant::now();
    let due: Vec<(Key, Params)> = {
        let mut st = lock(&hub.state);
        st.cache.retain(|_, e| e.last_touch + CACHE_IDLE > now);
        let mut seen = HashSet::new();
        st.subs
            .values()
            .filter(|s| seen.insert(s.key.clone()))
            .filter(|s| st_due(&st.cache, &s.key, now))
            .map(|s| (s.key.clone(), s.params.clone()))
            .collect()
    };
    for (key, params) in due {
        let Some(snap) = hub.refresh(&key, &params) else {
            continue;
        };
        push_to_subscribers(shared, &key, &snap);
    }
}

fn st_due(cache: &HashMap<Key, Entry>, key: &Key, now: Instant) -> bool {
    cache.get(key).is_none_or(|e| e.next_due <= now)
}

fn push_to_subscribers(shared: &Shared, key: &Key, snap: &Snap) {
    let targets: Vec<(u64, String, SyncSender<String>)> = lock(&shared.hub.state)
        .subs
        .iter()
        .filter(|(_, s)| &s.key == key)
        .map(|(id, s)| (*id, s.widget.clone(), s.tx.clone()))
        .collect();
    let mut dead = Vec::new();
    for (id, widget, tx) in targets {
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
        let data = grant.filter(snap.data.clone());
        match tx.try_send(push_line(id, &key.topic, snap, &data)) {
            Ok(()) => {}
            // Slow or gone consumer: drop the subscription rather than buffer without bound.
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => dead.push(id),
        }
    }
    shared.hub.drop_subs(&dead);
}
