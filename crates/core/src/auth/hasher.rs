//! Argon2id password hashing (PHC string format).

use argon2::password_hash::Error as PhcError;
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher as _, PasswordVerifier as _, Version,
};

use crate::account::{NewPassword, PasswordAttempt};

/// Argon2id cost parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashCost {
    /// Memory in KiB.
    pub memory_kib: u32,
    /// Passes over the memory.
    pub iterations: u32,
    /// Lanes.
    pub parallelism: u32,
}

impl HashCost {
    /// OWASP's first recommended Argon2id configuration: 19 MiB, two passes,
    /// one lane (about 20–60 ms per hash on a server core).
    pub const RECOMMENDED: Self = Self {
        memory_kib: 19 * 1024,
        iterations: 2,
        parallelism: 1,
    };

    /// The cheapest parameters Argon2 accepts, so that tests run quickly.
    /// Never for stored passwords.
    pub const MINIMUM_FOR_TESTS: Self = Self {
        memory_kib: 8,
        iterations: 1,
        parallelism: 1,
    };
}

/// Why hashing or verifying failed. A wrong password is not an error but
/// [`Verification::Mismatch`].
#[derive(Debug, thiserror::Error)]
pub enum HashError {
    /// The cost parameters are outside Argon2's limits.
    #[error("invalid Argon2 parameters: {0}")]
    Params(argon2::Error),
    /// Hashing failed (for example, the random source for the salt).
    #[error("password hashing failed: {0}")]
    Hash(PhcError),
    /// A stored hash is not a PHC string.
    #[error("the stored password hash is not a PHC string: {0}")]
    StoredFormat(argon2::password_hash::phc::Error),
    /// A stored hash names an algorithm or parameters Argon2 cannot verify
    /// against.
    #[error("the stored password hash is unusable: {0}")]
    StoredHash(PhcError),
    /// Hashing an address for the ban list failed.
    #[error("hashing an address for the ban list failed: {0}")]
    BanDigest(argon2::Error),
}

/// A password hash as stored: an Argon2id PHC string carrying its own
/// salt and parameters, so a later change of [`HashCost`] keeps old hashes
/// verifiable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPasswordHash(String);

impl StoredPasswordHash {
    /// A hash read back from storage; it is checked when used.
    #[must_use]
    pub const fn from_stored(phc: String) -> Self {
        Self(phc)
    }

    /// The PHC string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The outcome of checking a password against a hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// The password is the one hashed.
    Match,
    /// It is not.
    Mismatch,
}

/// Hashes and verifies passwords with Argon2id.
///
/// Hashing is CPU- and memory-bound for tens of milliseconds; async callers
/// run it on a blocking thread and bound how many run at once.
pub struct PasswordHasher {
    argon2: Argon2<'static>,
    /// A hash of a fixed text at the configured cost, verified against when
    /// a login names no account, so that it takes as long as a wrong
    /// password and does not reveal whether the account exists.
    decoy: StoredPasswordHash,
}

impl std::fmt::Debug for PasswordHasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordHasher").finish_non_exhaustive()
    }
}

impl PasswordHasher {
    /// A hasher at `cost`.
    pub fn new(cost: HashCost) -> Result<Self, HashError> {
        let params = Params::new(cost.memory_kib, cost.iterations, cost.parallelism, None)
            .map_err(HashError::Params)?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let decoy = argon2
            .hash_password(b"decoy for unknown accounts")
            .map_err(HashError::Hash)?;
        Ok(Self {
            argon2,
            decoy: StoredPasswordHash(decoy.to_string()),
        })
    }

    /// Hashes `password` with a fresh random salt.
    pub fn hash(&self, password: &NewPassword) -> Result<StoredPasswordHash, HashError> {
        let hash = self
            .argon2
            .hash_password(password.expose().as_bytes())
            .map_err(HashError::Hash)?;
        Ok(StoredPasswordHash(hash.to_string()))
    }

    /// Checks `attempt` against `stored`.
    pub fn verify(
        &self,
        attempt: &PasswordAttempt,
        stored: &StoredPasswordHash,
    ) -> Result<Verification, HashError> {
        let parsed = PasswordHash::new(stored.as_str()).map_err(HashError::StoredFormat)?;
        match self
            .argon2
            .verify_password(attempt.expose().as_bytes(), &parsed)
        {
            Ok(()) => Ok(Verification::Match),
            Err(PhcError::PasswordInvalid) => Ok(Verification::Mismatch),
            Err(other) => Err(HashError::StoredHash(other)),
        }
    }

    /// Spends one verification's time without an account (see `decoy`).
    pub fn verify_without_account(&self, attempt: &PasswordAttempt) -> Result<(), HashError> {
        self.verify(attempt, &self.decoy).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Email;

    fn hasher() -> PasswordHasher {
        PasswordHasher::new(HashCost::MINIMUM_FOR_TESTS).unwrap()
    }

    fn password(text: &str) -> NewPassword {
        NewPassword::parse(text.to_owned(), &Email::parse("lilei@qq.com").unwrap()).unwrap()
    }

    #[test]
    fn a_hash_verifies_only_its_password() {
        let hasher = hasher();
        let stored = hasher.hash(&password("correct horse battery")).unwrap();
        assert!(
            stored.as_str().starts_with("$argon2id$v=19$m=8,t=1,p=1$"),
            "{}",
            stored.as_str()
        );
        let right = PasswordAttempt::new("correct horse battery".to_owned());
        let wrong = PasswordAttempt::new("correct horse battery!".to_owned());
        assert_eq!(hasher.verify(&right, &stored).unwrap(), Verification::Match);
        assert_eq!(
            hasher.verify(&wrong, &stored).unwrap(),
            Verification::Mismatch
        );
    }

    #[test]
    fn hashes_are_salted() {
        let hasher = hasher();
        let first = hasher.hash(&password("correct horse battery")).unwrap();
        let second = hasher.hash(&password("correct horse battery")).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn the_recommended_cost_is_owasps_and_verifies_across_costs() {
        assert_eq!(
            HashCost::RECOMMENDED,
            HashCost {
                memory_kib: 19_456,
                iterations: 2,
                parallelism: 1
            }
        );
        // A hash keeps its own parameters: a hasher at another cost still
        // verifies it.
        let stored = hasher().hash(&password("correct horse battery")).unwrap();
        let other = PasswordHasher::new(HashCost {
            memory_kib: 16,
            iterations: 2,
            parallelism: 1,
        })
        .unwrap();
        let attempt = PasswordAttempt::new("correct horse battery".to_owned());
        assert_eq!(
            other.verify(&attempt, &stored).unwrap(),
            Verification::Match
        );
    }

    #[test]
    fn unusable_hashes_and_parameters_are_errors() {
        let attempt = PasswordAttempt::new("anything at all".to_owned());
        let garbage = StoredPasswordHash::from_stored("not a phc string".to_owned());
        assert!(matches!(
            hasher().verify(&attempt, &garbage),
            Err(HashError::StoredFormat(_))
        ));
        // A four-byte output is shorter than any PHC hash.
        let short = StoredPasswordHash::from_stored(
            "$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$aGFzaA".to_owned(),
        );
        assert!(matches!(
            hasher().verify(&attempt, &short),
            Err(HashError::StoredFormat(_))
        ));
        // A well-formed PHC string of an algorithm Argon2 does not verify.
        let scrypt = format!("$scrypt$ln=1,r=8,p=1$c2FsdHNhbHQ${}", "A".repeat(43));
        let unknown = StoredPasswordHash::from_stored(scrypt);
        assert!(matches!(
            hasher().verify(&attempt, &unknown),
            Err(HashError::StoredHash(_))
        ));
        assert!(matches!(
            PasswordHasher::new(HashCost {
                memory_kib: 1,
                iterations: 1,
                parallelism: 1
            }),
            Err(HashError::Params(_))
        ));
    }

    #[test]
    fn a_login_without_an_account_still_verifies() {
        let hasher = hasher();
        let attempt = PasswordAttempt::new("decoy for unknown accounts".to_owned());
        hasher.verify_without_account(&attempt).unwrap();
        assert_eq!(format!("{hasher:?}"), "PasswordHasher { .. }");
    }
}
