//! Fixed-window rate limits over a bounded set of keys.
//!
//! A limiter counts actions per key (a client network, an account) in
//! windows that open with the key's first action. It tracks at most
//! `capacity` keys: when it is full it drops the keys whose window has
//! closed, and if none has, it refuses to track a new key
//! ([`Saturated`]), which callers treat as limited: under a flood from
//! more networks than that, refusing service is the bounded failure.

use std::collections::HashMap;
use std::hash::Hash;

use crate::time::UnixTime;

/// How many actions a key may take per window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatePolicy {
    /// Actions allowed per window.
    pub limit: u32,
    /// The window's length in seconds.
    pub window_secs: u64,
}

/// Whether a key may act now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// It may.
    Allowed,
    /// It has used its window's allowance.
    Limited {
        /// Seconds until the window closes.
        retry_after_secs: u64,
    },
}

/// The limiter is full of open windows and cannot track another key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the rate limiter already tracks {capacity} keys with open windows")]
pub struct Saturated {
    /// How many keys it tracks.
    pub capacity: usize,
}

#[derive(Debug, Clone, Copy)]
struct Window {
    opened: UnixTime,
    count: u32,
}

/// Counts actions per key; see the module documentation.
#[derive(Debug)]
pub struct RateLimiter<K> {
    policy: RatePolicy,
    capacity: usize,
    windows: HashMap<K, Window>,
}

impl<K: Eq + Hash> RateLimiter<K> {
    /// A limiter enforcing `policy` on at most `capacity` keys at a time.
    #[must_use]
    pub fn new(policy: RatePolicy, capacity: usize) -> Self {
        Self {
            policy,
            capacity,
            windows: HashMap::new(),
        }
    }

    /// Whether `key` may act at `now`; counts nothing.
    #[must_use]
    pub fn check(&self, key: &K, now: UnixTime) -> Decision {
        match self.windows.get(key) {
            Some(window) if self.is_open(window, now) && window.count >= self.policy.limit => {
                Decision::Limited {
                    retry_after_secs: self.closes_at(window).secs_since(now),
                }
            }
            _ => Decision::Allowed,
        }
    }

    /// Counts one action by `key` at `now`.
    pub fn record(&mut self, key: K, now: UnixTime) -> Result<(), Saturated> {
        let open = self
            .windows
            .get(&key)
            .is_some_and(|window| self.is_open(window, now));
        if open {
            if let Some(window) = self.windows.get_mut(&key) {
                window.count = window.count.saturating_add(1);
            }
            return Ok(());
        }
        if !self.windows.contains_key(&key) && self.windows.len() >= self.capacity {
            self.drop_closed(now);
            if self.windows.len() >= self.capacity {
                return Err(Saturated {
                    capacity: self.capacity,
                });
            }
        }
        self.windows.insert(
            key,
            Window {
                opened: now,
                count: 1,
            },
        );
        Ok(())
    }

    /// [`Self::check`], then [`Self::record`] when allowed.
    pub fn acquire(&mut self, key: K, now: UnixTime) -> Result<Decision, Saturated> {
        let decision = self.check(&key, now);
        if decision == Decision::Allowed {
            self.record(key, now)?;
        }
        Ok(decision)
    }

    /// Forgets `key`, as after a successful login.
    pub fn forget(&mut self, key: &K) {
        self.windows.remove(key);
    }

    /// How many keys are tracked.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.windows.len()
    }

    fn drop_closed(&mut self, now: UnixTime) {
        let window_secs = self.policy.window_secs;
        self.windows
            .retain(|_, window| now.secs_since(window.opened) < window_secs);
    }

    fn is_open(&self, window: &Window, now: UnixTime) -> bool {
        now.secs_since(window.opened) < self.policy.window_secs
    }

    fn closes_at(&self, window: &Window) -> UnixTime {
        window
            .opened
            .checked_add_secs(self.policy.window_secs)
            .unwrap_or(UnixTime::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: RatePolicy = RatePolicy {
        limit: 3,
        window_secs: 60,
    };

    fn at(secs: u64) -> UnixTime {
        UnixTime::from_secs(secs).unwrap()
    }

    #[test]
    fn a_key_is_limited_after_its_allowance_until_the_window_closes() {
        let mut limiter = RateLimiter::new(POLICY, 10);
        for second in [100, 110, 120] {
            assert_eq!(limiter.acquire("a", at(second)), Ok(Decision::Allowed));
        }
        assert_eq!(
            limiter.acquire("a", at(130)),
            Ok(Decision::Limited {
                retry_after_secs: 30
            })
        );
        assert_eq!(
            limiter.check(&"a", at(159)),
            Decision::Limited {
                retry_after_secs: 1
            }
        );
        // The window opened at 100 closes at 160.
        assert_eq!(limiter.acquire("a", at(160)), Ok(Decision::Allowed));
        // Other keys are unaffected.
        assert_eq!(limiter.check(&"b", at(130)), Decision::Allowed);
    }

    #[test]
    fn check_counts_nothing_and_record_counts_without_checking() {
        let mut limiter = RateLimiter::new(POLICY, 10);
        for _ in 0..5 {
            assert_eq!(limiter.check(&"a", at(0)), Decision::Allowed);
        }
        for _ in 0..3 {
            limiter.record("a", at(0)).unwrap();
        }
        assert!(matches!(
            limiter.check(&"a", at(1)),
            Decision::Limited { .. }
        ));
        limiter.forget(&"a");
        assert_eq!(limiter.check(&"a", at(1)), Decision::Allowed);
        assert_eq!(limiter.tracked(), 0);
    }

    #[test]
    fn a_full_limiter_drops_closed_windows_then_refuses() {
        let mut limiter = RateLimiter::new(POLICY, 2);
        limiter.record(1, at(0)).unwrap();
        limiter.record(2, at(30)).unwrap();
        // Both windows are open: a third key cannot be tracked.
        assert_eq!(limiter.record(3, at(59)), Err(Saturated { capacity: 2 }));
        assert_eq!(limiter.acquire(3, at(59)), Err(Saturated { capacity: 2 }));
        // A tracked key still counts.
        limiter.record(1, at(59)).unwrap();
        // Key 1's window has closed by 60, so it makes room.
        limiter.record(3, at(60)).unwrap();
        assert_eq!(limiter.tracked(), 2);
        assert_eq!(limiter.check(&1, at(60)), Decision::Allowed);
    }

    #[test]
    fn a_closed_window_reopens_on_the_next_action() {
        let mut limiter = RateLimiter::new(POLICY, 1);
        for _ in 0..3 {
            limiter.record("a", at(0)).unwrap();
        }
        // The same key reuses its slot even when the limiter is full.
        limiter.record("a", at(500)).unwrap();
        assert_eq!(limiter.check(&"a", at(500)), Decision::Allowed);
        limiter.record("a", at(500)).unwrap();
        limiter.record("a", at(500)).unwrap();
        assert_eq!(
            limiter.check(&"a", at(500)),
            Decision::Limited {
                retry_after_secs: 60
            }
        );
    }

    #[test]
    fn windows_near_the_end_of_time_close_at_the_last_instant() {
        let mut limiter = RateLimiter::new(
            RatePolicy {
                limit: 1,
                window_secs: u64::MAX,
            },
            1,
        );
        limiter.record("a", UnixTime::MAX).unwrap();
        assert_eq!(
            limiter.check(&"a", UnixTime::MAX),
            Decision::Limited {
                retry_after_secs: 0
            }
        );
    }
}
