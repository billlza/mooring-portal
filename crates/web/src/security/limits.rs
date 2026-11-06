//! The portal's rate limits on logins and registrations.
//!
//! A network may attempt [`LOGIN_PER_NETWORK`] logins and
//! [`REGISTRATIONS_PER_NETWORK`] registrations per window, and an email may
//! have its password checked [`PASSWORD_CHECKS_PER_EMAIL`] times per window,
//! whether or not an account has it, so the limit itself reveals nothing. A
//! correct password clears the email's count, so in effect the limit is on
//! failures. Each check is counted before the password is hashed, under the
//! limiter's lock, so concurrent guesses cannot all pass a count that has
//! not yet been raised. A limiter that cannot track another key refuses
//! (see `portal_core::rate_limit`).

use std::sync::{Mutex, MutexGuard};

use portal_core::account::Email;
use portal_core::rate_limit::{Decision, RateLimiter, RatePolicy};
use portal_core::time::UnixTime;

use super::client_ip::Network;
use crate::error::AppError;

/// Login attempts per client network.
pub(crate) const LOGIN_PER_NETWORK: RatePolicy = RatePolicy {
    limit: 20,
    window_secs: 15 * 60,
};
/// Password checks per email, at login or password change, until one
/// succeeds.
pub(crate) const PASSWORD_CHECKS_PER_EMAIL: RatePolicy = RatePolicy {
    limit: 10,
    window_secs: 15 * 60,
};
/// Registration attempts per client network.
pub(crate) const REGISTRATIONS_PER_NETWORK: RatePolicy = RatePolicy {
    limit: 10,
    window_secs: 60 * 60,
};
/// Keys each limiter tracks at most.
const TRACKED_KEYS: usize = 10_000;

/// Whether an attempt may go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Throttle {
    /// It may.
    Allowed,
    /// Not for this many minutes.
    Wait {
        /// Whole minutes, at least one.
        minutes: u64,
    },
}

impl Throttle {
    fn of(decision: Decision) -> Self {
        match decision {
            Decision::Allowed => Self::Allowed,
            Decision::Limited { retry_after_secs } => Self::Wait {
                minutes: retry_after_secs.div_ceil(60).max(1),
            },
        }
    }

    fn saturated(policy: RatePolicy) -> Self {
        tracing::warn!("a rate limiter is full of open windows; refusing until some close");
        Self::Wait {
            minutes: policy.window_secs.div_ceil(60),
        }
    }
}

/// The rate limiters, shared by every request.
#[derive(Debug)]
pub(crate) struct Limits {
    logins: Mutex<RateLimiter<Network>>,
    password_checks: Mutex<RateLimiter<String>>,
    registrations: Mutex<RateLimiter<Network>>,
}

impl Limits {
    pub(crate) fn new() -> Self {
        Self {
            logins: Mutex::new(RateLimiter::new(LOGIN_PER_NETWORK, TRACKED_KEYS)),
            password_checks: Mutex::new(RateLimiter::new(PASSWORD_CHECKS_PER_EMAIL, TRACKED_KEYS)),
            registrations: Mutex::new(RateLimiter::new(REGISTRATIONS_PER_NETWORK, TRACKED_KEYS)),
        }
    }

    /// Counts a login attempt from `network`.
    pub(crate) fn login_attempt(
        &self,
        network: Network,
        now: UnixTime,
    ) -> Result<Throttle, AppError> {
        Ok(match lock(&self.logins)?.acquire(network, now) {
            Ok(decision) => Throttle::of(decision),
            Err(_) => Throttle::saturated(LOGIN_PER_NETWORK),
        })
    }

    /// Counts a registration attempt from `network`.
    pub(crate) fn registration_attempt(
        &self,
        network: Network,
        now: UnixTime,
    ) -> Result<Throttle, AppError> {
        Ok(match lock(&self.registrations)?.acquire(network, now) {
            Ok(decision) => Throttle::of(decision),
            Err(_) => Throttle::saturated(REGISTRATIONS_PER_NETWORK),
        })
    }

    /// Counts a password check for `email`, to be made only if allowed.
    pub(crate) fn password_attempt(
        &self,
        email: &Email,
        now: UnixTime,
    ) -> Result<Throttle, AppError> {
        Ok(
            match lock(&self.password_checks)?.acquire(email.as_str().to_owned(), now) {
                Ok(decision) => Throttle::of(decision),
                Err(_) => Throttle::saturated(PASSWORD_CHECKS_PER_EMAIL),
            },
        )
    }

    /// Clears `email`'s count after a correct password.
    pub(crate) fn password_succeeded(&self, email: &Email) -> Result<(), AppError> {
        lock(&self.password_checks)?.forget(&email.as_str().to_owned());
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, AppError> {
    mutex
        .lock()
        .map_err(|_| AppError::internal_message("a rate limiter's lock is poisoned"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> UnixTime {
        UnixTime::from_secs(secs).unwrap()
    }

    #[test]
    fn logins_are_limited_per_network() {
        let limits = Limits::new();
        let home = Network::V4([203, 0, 113, 1]);
        for _ in 0..LOGIN_PER_NETWORK.limit {
            assert_eq!(
                limits.login_attempt(home, at(1000)).unwrap(),
                Throttle::Allowed
            );
        }
        assert_eq!(
            limits.login_attempt(home, at(1000)).unwrap(),
            Throttle::Wait { minutes: 15 }
        );
        // Ninety seconds before the window closes, two minutes are asked.
        assert_eq!(
            limits.login_attempt(home, at(1000 + 15 * 60 - 90)).unwrap(),
            Throttle::Wait { minutes: 2 }
        );
        assert_eq!(
            limits
                .login_attempt(Network::V4([203, 0, 113, 2]), at(1000))
                .unwrap(),
            Throttle::Allowed
        );
    }

    #[test]
    fn password_checks_are_limited_per_email_and_cleared_on_success() {
        let limits = Limits::new();
        let email = Email::parse("a@qq.com").unwrap();
        for _ in 0..PASSWORD_CHECKS_PER_EMAIL.limit {
            assert_eq!(
                limits.password_attempt(&email, at(0)).unwrap(),
                Throttle::Allowed
            );
        }
        assert_eq!(
            limits.password_attempt(&email, at(1)).unwrap(),
            Throttle::Wait { minutes: 15 }
        );
        limits.password_succeeded(&email).unwrap();
        assert_eq!(
            limits.password_attempt(&email, at(1)).unwrap(),
            Throttle::Allowed
        );
    }

    #[test]
    fn registrations_are_limited_per_network() {
        let limits = Limits::new();
        let home = Network::V6([0x20, 0x01, 0x0d, 0xb8, 0, 1]);
        for _ in 0..REGISTRATIONS_PER_NETWORK.limit {
            assert_eq!(
                limits.registration_attempt(home, at(0)).unwrap(),
                Throttle::Allowed
            );
        }
        assert_eq!(
            limits.registration_attempt(home, at(0)).unwrap(),
            Throttle::Wait { minutes: 60 }
        );
    }

    #[test]
    fn a_full_limiter_refuses_new_networks_and_emails() {
        let limits = Limits::new();
        let now = at(0);
        for index in 0..u32::try_from(TRACKED_KEYS).unwrap() {
            let network = Network::V4(index.to_be_bytes());
            assert_eq!(
                limits.login_attempt(network, now).unwrap(),
                Throttle::Allowed
            );
            assert_eq!(
                limits.registration_attempt(network, now).unwrap(),
                Throttle::Allowed
            );
            let email = Email::parse(&format!("user{index}@qq.com")).unwrap();
            assert_eq!(
                limits.password_attempt(&email, now).unwrap(),
                Throttle::Allowed
            );
        }
        let stranger = Network::V4([255, 255, 255, 255]);
        assert_eq!(
            limits.login_attempt(stranger, now).unwrap(),
            Throttle::Wait { minutes: 15 }
        );
        assert_eq!(
            limits.registration_attempt(stranger, now).unwrap(),
            Throttle::Wait { minutes: 60 }
        );
        let email = Email::parse("stranger@qq.com").unwrap();
        assert_eq!(
            limits.password_attempt(&email, now).unwrap(),
            Throttle::Wait { minutes: 15 }
        );
        // Once the windows close, there is room again.
        assert_eq!(
            limits.login_attempt(stranger, at(15 * 60)).unwrap(),
            Throttle::Allowed
        );
    }

    #[test]
    fn saturation_refuses_for_a_whole_window() {
        assert_eq!(
            Throttle::saturated(LOGIN_PER_NETWORK),
            Throttle::Wait { minutes: 15 }
        );
        assert_eq!(
            Throttle::of(Decision::Limited {
                retry_after_secs: 0
            }),
            Throttle::Wait { minutes: 1 }
        );
    }
}
