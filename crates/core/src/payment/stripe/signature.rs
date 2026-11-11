//! Whether a delivery came from Stripe.
//!
//! Stripe signs each delivery: the `Stripe-Signature` header carries the
//! time of signing (`t`) and one or more signatures (`v1`), each the
//! HMAC-SHA256, in hexadecimal, of the time, a full stop and the request's
//! body exactly as sent, under the endpoint's signing secret. While a
//! secret is being rolled Stripe signs with the old and the new, so the
//! portal may hold two and accepts a delivery either signs.
//!
//! What this defends against: anyone can post to the portal's address, and
//! a forged "paid" would be granted service by Mooring. A forger without
//! the secret cannot make a signature; one who captures a real delivery
//! can replay it only within [`TOLERANCE_SECS`], and the portal acts on
//! each event's id once.

use std::fmt;

use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;

use crate::time::UnixTime;

/// How far a delivery's time of signing may be from the portal's clock,
/// either way: five minutes, as Stripe's own libraries allow.
pub const TOLERANCE_SECS: u64 = 300;
/// What every signing secret begins with.
const PREFIX: &str = "whsec_";

/// An endpoint's signing secret, as Stripe's Dashboard shows it
/// (`whsec_…`). The whole text is the key. Its `Debug` output is redacted.
#[derive(Clone)]
pub struct SigningSecret(String);

/// Text that is not a signing secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a signing secret: one begins with whsec_ and is letters, digits and _ after it")]
pub struct SecretError;

impl SigningSecret {
    /// Validates `text` as a signing secret.
    pub fn parse(text: &str) -> Result<Self, SecretError> {
        let valid = text.strip_prefix(PREFIX).is_some_and(|rest| {
            (16..=250).contains(&rest.len())
                && rest
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        });
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(SecretError)
        }
    }
}

impl fmt::Debug for SigningSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SigningSecret(<redacted>)")
    }
}

/// Why a delivery is not believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    /// The header is not a time and at least one signature.
    #[error("the Stripe-Signature header is not a time and a signature")]
    Header,
    /// No signature in the header is this body's under any secret held.
    #[error("no signature matches the body under a signing secret the portal holds")]
    Mismatch,
    /// Signed by a secret the portal holds, but too long ago or too far
    /// ahead: a replay, or a clock that is wrong.
    #[error("the delivery was signed more than {TOLERANCE_SECS} seconds from now")]
    Stale,
}

/// Checks that `body` was signed, as `header` says, under one of
/// `secrets`, within [`TOLERANCE_SECS`] of `now`.
pub fn verify(
    secrets: &[SigningSecret],
    header: &str,
    body: &[u8],
    now: UnixTime,
) -> Result<(), SignatureError> {
    let mut time = None;
    let mut signatures = Vec::new();
    for entry in header.split(',') {
        match entry.split_once('=') {
            Some(("t", value)) if time.is_none() => {
                time = Some(seconds(value).ok_or(SignatureError::Header)?);
            }
            // A second time would make it unclear which was signed.
            Some(("t", _)) => return Err(SignatureError::Header),
            Some(("v1", value)) => signatures.push(value),
            // Other schemes (`v0`, which Stripe sends in sandboxes) and
            // anything else are not signatures the portal reads.
            _ => {}
        }
    }
    let (Some(time), false) = (time, signatures.is_empty()) else {
        return Err(SignatureError::Header);
    };
    let signed: Vec<[u8; 32]> = signatures.iter().filter_map(|text| digest(text)).collect();
    // HMAC takes a key of any length, so every secret makes one.
    let matches = secrets
        .iter()
        .filter_map(|secret| Hmac::<Sha256>::new_from_slice(secret.0.as_bytes()).ok())
        .any(|mut mac| {
            mac.update(time.to_string().as_bytes());
            mac.update(b".");
            mac.update(body);
            // Every signature is compared, each in constant time.
            signed.iter().fold(false, |found, signature| {
                found | mac.clone().verify_slice(signature).is_ok()
            })
        });
    if !matches {
        return Err(SignatureError::Mismatch);
    }
    if time.abs_diff(now.as_secs()) > TOLERANCE_SECS {
        return Err(SignatureError::Stale);
    }
    Ok(())
}

/// A time as the header writes one: decimal digits, without sign or
/// space.
fn seconds(text: &str) -> Option<u64> {
    let plain = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    plain.then(|| text.parse().ok()).flatten()
}

/// A signature as the header writes one: 64 lowercase hexadecimal digits.
fn digest(text: &str) -> Option<[u8; 32]> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if pairs.len() != 32 || !rest.is_empty() {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
        *byte = (hex(*high)? << 4) | hex(*low)?;
    }
    Some(bytes)
}

const fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
