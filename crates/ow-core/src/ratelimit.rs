use std::collections::HashMap;
use std::time::Instant;

/// Token bucket per key. `now` is a parameter so tests need no sleeping.
pub struct RateLimiter {
    capacity: f64,
    per_sec: f64,
    buckets: HashMap<(String, String), (f64, Instant)>,
}

/// Bound on tracked keys so a client inventing widget ids cannot grow memory forever.
const MAX_KEYS: usize = 4096;

impl RateLimiter {
    pub fn new(capacity: u32, per_sec: f64) -> Self {
        Self {
            capacity: f64::from(capacity),
            per_sec,
            buckets: HashMap::new(),
        }
    }

    pub fn allow(&mut self, widget: &str, topic: &str, now: Instant) -> bool {
        if self.buckets.len() >= MAX_KEYS {
            // Forget buckets that have fully refilled; they behave like fresh ones.
            let (cap, rate) = (self.capacity, self.per_sec);
            self.buckets.retain(|_, (t, at)| {
                *t + now.saturating_duration_since(*at).as_secs_f64() * rate < cap
            });
        }
        let (tokens, at) = self
            .buckets
            .entry((widget.to_owned(), topic.to_owned()))
            .or_insert((self.capacity, now));
        let elapsed = now.saturating_duration_since(*at);
        *tokens = (*tokens + elapsed.as_secs_f64() * self.per_sec).min(self.capacity);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn burst_then_limited_then_refills() {
        let mut r = RateLimiter::new(3, 1.0);
        let t0 = Instant::now();
        assert!((0..3).all(|_| r.allow("w", "t", t0)));
        assert!(!r.allow("w", "t", t0));
        assert!(r.allow("w", "t", t0 + Duration::from_secs(1)));
        assert!(!r.allow("w", "t", t0 + Duration::from_secs(1)));
    }

    #[test]
    fn keys_are_independent() {
        let mut r = RateLimiter::new(1, 0.0);
        let t = Instant::now();
        assert!(r.allow("a", "t", t));
        assert!(r.allow("b", "t", t));
        assert!(!r.allow("a", "t", t));
    }
}
