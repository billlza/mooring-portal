//! What the ban list keeps of a banned address.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};

use crate::account::Email;

use super::HashError;

/// The cost of a ban digest: 19 MiB, two passes, one lane, the cost OWASP
/// recommends for passwords, so that each candidate address tried against
/// a copy of the list costs as much as one password guess (one guess tests
/// every entry at once: the salt is shared). It is fixed, not configured:
/// the server and the operator's commands must compute the same digest,
/// and a change would orphan every ban recorded before it.
const MEMORY_KIB: u32 = 19 * 1024;
const ITERATIONS: u32 = 2;
const PARALLELISM: u32 = 1;

/// The salt, fixed for the same reason. Being public, it only keeps tables
/// computed for other uses of Argon2id from applying to this list.
const SALT: &[u8] = b"mooring-portal ban list v1";

/// What the ban list keeps of a banned address: an Argon2id hash of the
/// address as [`Email::parse`] normalises it.
///
/// The list must outlive the account it came from, and a ban implies an
/// accusation, so the list does not keep the addresses themselves. The
/// hash is slow on purpose. Anyone who holds a copy of the list can still
/// test a given address, but each candidate costs tens of milliseconds:
/// running through every numeric QQ address (fewer than 10^10) takes some
/// core-years, out of casual reach though not of someone who rents the
/// computers. A key kept out of the database would put it out of reach
/// altogether; there is none.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BanDigest([u8; 32]);

impl BanDigest {
    /// The digest of `email`. Two addresses share it only when they
    /// normalise alike: surrounding white space and letter case are
    /// ignored, nothing else. Dots and `+` suffixes in the local part are
    /// kept, and no two domains are taken for one, because those rules
    /// hold for some mail services only, and elsewhere they would make two
    /// people one.
    ///
    /// Takes tens of milliseconds and 19 MiB: async callers run it on a
    /// blocking thread, as they do password hashing.
    pub fn of(email: &Email) -> Result<Self, HashError> {
        let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, Some(32))
            .map_err(HashError::Params)?;
        let mut digest = [0; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(email.as_str().as_bytes(), SALT, &mut digest)
            .map_err(HashError::BanDigest)?;
        Ok(Self(digest))
    }

    /// The digest's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// An address together with the ban list's digest of it. Only
/// [`DigestedEmail::new`] makes one, from the address itself, so a
/// registration cannot check the list with another address's digest.
#[derive(Debug, Clone)]
pub struct DigestedEmail {
    email: Email,
    digest: BanDigest,
}

impl DigestedEmail {
    /// Digests `email` for the ban list, as slowly as [`BanDigest::of`].
    pub fn new(email: Email) -> Result<Self, HashError> {
        let digest = BanDigest::of(&email)?;
        Ok(Self { email, digest })
    }

    /// The address.
    #[must_use]
    pub const fn email(&self) -> &Email {
        &self.email
    }

    /// The ban list's digest of the address.
    #[must_use]
    pub const fn ban_digest(&self) -> &BanDigest {
        &self.digest
    }

    /// The address, without its digest.
    #[must_use]
    pub fn into_email(self) -> Email {
        self.email
    }
}

impl fmt::Debug for BanDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BanDigest(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(text: &str) -> BanDigest {
        BanDigest::of(&Email::parse(text).unwrap()).unwrap()
    }

    #[test]
    fn the_digest_is_argon2id_at_the_fixed_cost_and_salt() {
        let mut expected = [0; 32];
        Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(19_456, 2, 1, Some(32)).unwrap(),
        )
        .hash_password_into(
            b"li.lei+portal@qq.com",
            b"mooring-portal ban list v1",
            &mut expected,
        )
        .unwrap();
        assert_eq!(digest("Li.Lei+Portal@QQ.com").as_bytes(), &expected);
    }

    #[test]
    fn the_digest_of_a_known_address_never_changes() {
        // Pinned: a change of cost, salt or normalisation would let every
        // address banned before it register again.
        let hex: String = digest("Li.Lei+Portal@QQ.com")
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            hex,
            "47913323b9af5e913791d579362596ffb0fb70cb7ef25424f8280f95a648ad00"
        );
    }

    #[test]
    fn the_digest_ignores_only_case_and_white_space() {
        let banned = digest("Li.Lei+Portal@QQ.com");
        assert_eq!(banned, digest("  li.lei+portal@qq.COM \n"));
        // Other people's addresses, under rules some services apply.
        for other in [
            "lilei+portal@qq.com",
            "li.lei@qq.com",
            "li.lei+other@qq.com",
            "li.lei+portal@foxmail.com",
        ] {
            assert_ne!(banned, digest(other), "{other}");
        }
        assert_eq!(format!("{banned:?}"), "BanDigest(..)");
    }

    #[test]
    fn a_digested_address_carries_its_own_digest() {
        let email = Email::parse("Li.Lei@QQ.com").unwrap();
        let digested = DigestedEmail::new(email.clone()).unwrap();
        assert_eq!(digested.email(), &email);
        assert_eq!(digested.ban_digest(), &BanDigest::of(&email).unwrap());
        assert_eq!(digested.into_email(), email);
    }
}
