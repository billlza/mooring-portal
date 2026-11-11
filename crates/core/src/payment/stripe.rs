//! What Stripe tells the portal: signed events about payments made on the
//! operator's Payment Links.
//!
//! An event is believed only when it is signed with a secret the portal
//! shares with Stripe ([`verify`]), and is then read for a handful of
//! values ([`Event`]). Stripe renders an event for one version of its API;
//! the portal reads the shapes of one release of it ([`API_RELEASE`]) and
//! refuses any other, rather than guess at a shape it was not written
//! for.

mod event;
mod signature;

use std::fmt;

use blake2::{Blake2s256, Digest as _};
use serde::Deserialize;

pub use event::{
    API_RELEASE, BillingReason, CardUsed, Charge, CheckoutMode, CheckoutSession, Dispute, Event,
    EventError, EventKind, Invoice, InvoicePayment, MAX_EVENT_BYTES, PaymentStatus, Refunded,
    Renewal, SubscriptionChange,
};
pub use signature::{SecretError, SignatureError, SigningSecret, TOLERANCE_SECS, verify};

/// An id as Stripe writes one: 1 to 255 ASCII letters, digits and
/// underscores. Stripe promises no more about an id's form than its
/// length, so nothing more is asked of one; the characters are those every
/// id has had, and keep an id safe to print.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct StripeId(String);

/// Text that is not an id as Stripe writes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not an id as Stripe writes one")]
pub struct StripeIdError;

impl StripeId {
    /// Validates `text` as an id.
    pub fn parse(text: &str) -> Result<Self, StripeIdError> {
        let valid = (1..=255).contains(&text.len())
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(StripeIdError)
        }
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for StripeId {
    type Error = StripeIdError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl fmt::Display for StripeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stripe's fingerprint of a card number: the same for every payment made
/// with that number on the operator's account, and meaningless anywhere
/// else. Its `Debug` output is redacted, so that it reaches no log.
#[derive(Clone, PartialEq, Eq)]
pub struct CardFingerprint(String);

/// Text that is not a card's fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a card fingerprint as Stripe writes one (8 to 64 letters and digits)")]
pub struct CardFingerprintError;

impl CardFingerprint {
    /// Validates `text` as a fingerprint: 8 to 64 ASCII letters and
    /// digits, as Stripe's Dashboard shows one.
    pub fn parse(text: &str) -> Result<Self, CardFingerprintError> {
        let valid =
            (8..=64).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_alphanumeric());
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(CardFingerprintError)
        }
    }

    /// The digest the ban list keeps in place of the fingerprint.
    #[must_use]
    pub fn digest(&self) -> CardDigest {
        CardDigest(Blake2s256::digest(self.0.as_bytes()).into())
    }
}

impl fmt::Debug for CardFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CardFingerprint(<redacted>)")
    }
}

/// The BLAKE2s-256 digest of a card's fingerprint.
///
/// A fingerprint is long and random, so a plain digest cannot be turned
/// back into one; the ban list keeps the digest so that the database does
/// not hold the identifier itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CardDigest([u8; 32]);

impl CardDigest {
    /// The digest's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_letters_digits_and_underscores() {
        for good in ["evt_1", "cs_test_a1B2", "x", &"a".repeat(255)] {
            assert_eq!(StripeId::parse(good).unwrap().as_str(), good);
            assert_eq!(StripeId::parse(good).unwrap().to_string(), good);
        }
        for bad in ["", "evt 1", "evt-1", "evt.1", "\u{e9}", &"a".repeat(256)] {
            assert_eq!(StripeId::parse(bad), Err(StripeIdError), "{bad:?}");
        }
        let read: StripeId = serde_json::from_str("\"pi_3\"").unwrap();
        assert_eq!(read.as_str(), "pi_3");
        assert!(serde_json::from_str::<StripeId>("\"pi 3\"").is_err());
        assert_eq!(StripeIdError.to_string(), "not an id as Stripe writes one");
    }

    #[test]
    fn a_fingerprint_is_kept_as_its_digest_and_never_printed() {
        let card = CardFingerprint::parse("Xt5EWLLDS7FJjR1c").unwrap();
        assert_eq!(format!("{card:?}"), "CardFingerprint(<redacted>)");
        // BLAKE2s-256 of the fingerprint's text, as another program
        // computes it.
        let digest = card.digest();
        let hex: String = digest
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            hex,
            "d766acaf1ae89724e10e34e0e0025a911d775c7fe9b8eb7eb9624c6d2e15fbad"
        );
        assert_eq!(
            digest,
            CardFingerprint::parse("Xt5EWLLDS7FJjR1c").unwrap().digest()
        );
        assert_ne!(
            digest,
            CardFingerprint::parse("Xt5EWLLDS7FJjR1d").unwrap().digest()
        );
        for bad in [
            "",
            "short",
            &"a".repeat(7),
            "Xt5EWLLDS7FJjR1!",
            "Xt5E WLLDS7FJjR1",
            &"a".repeat(65),
        ] {
            assert_eq!(
                CardFingerprint::parse(bad),
                Err(CardFingerprintError),
                "{bad:?}"
            );
        }
        assert!(CardFingerprint::parse(&"a".repeat(8)).is_ok());
        assert!(CardFingerprint::parse(&"a".repeat(64)).is_ok());
        assert!(
            CardFingerprintError
                .to_string()
                .starts_with("not a card fingerprint")
        );
    }
}
