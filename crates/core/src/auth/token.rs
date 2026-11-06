//! Random bearer tokens (sessions and CSRF) and their digests.

use std::fmt;

use blake2::{Blake2s256, Digest as _};
use subtle::ConstantTimeEq as _;

/// The bytes in a token: 256 bits.
pub const TOKEN_BYTES: usize = 32;

/// The operating system's random source failed.
#[derive(Debug, thiserror::Error)]
#[error("the operating system's random source failed: {0}")]
pub struct RandomError(#[from] getrandom::Error);

/// Text that is not a token: tokens are exactly 64 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a token (64 lowercase hexadecimal digits)")]
pub struct MalformedToken;

/// 256 bits from the operating system's random source, written as 64
/// lowercase hex digits. Its `Debug` output is redacted.
#[derive(Clone)]
pub struct SecretToken([u8; TOKEN_BYTES]);

impl SecretToken {
    /// A fresh token.
    pub fn generate() -> Result<Self, RandomError> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// Reads a token written by [`Self::encode`].
    pub fn parse(text: &str) -> Result<Self, MalformedToken> {
        let (pairs, rest) = text.as_bytes().as_chunks::<2>();
        if pairs.len() != TOKEN_BYTES || !rest.is_empty() {
            return Err(MalformedToken);
        }
        let mut bytes = [0_u8; TOKEN_BYTES];
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (hex_value(*high)? << 4) | hex_value(*low)?;
        }
        Ok(Self(bytes))
    }

    /// The token as 64 lowercase hex digits.
    #[must_use]
    pub fn encode(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(2 * TOKEN_BYTES);
        for byte in self.0 {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        text
    }

    /// The digest the server stores in place of a session token.
    #[must_use]
    pub fn digest(&self) -> TokenDigest {
        TokenDigest(Blake2s256::digest(self.0).into())
    }

    /// Whether `self` and `other` are the same token, compared in constant
    /// time.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken(<redacted>)")
    }
}

/// The BLAKE2s-256 digest of a session token.
///
/// The session table stores digests, not tokens, so a leaked copy of the
/// database (a backup, say) holds nothing a browser could present: the
/// tokens have 256 bits of entropy, so the digest cannot be inverted.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenDigest([u8; 32]);

impl TokenDigest {
    /// A digest read back from storage.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for TokenDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenDigest(..)")
    }
}

const fn hex_value(digit: u8) -> Result<u8, MalformedToken> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err(MalformedToken),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_round_trip_through_hex() {
        let token = SecretToken::generate().unwrap();
        let text = token.encode();
        assert_eq!(text.len(), 64);
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let parsed = SecretToken::parse(&text).unwrap();
        assert!(parsed.matches(&token));
        assert_eq!(parsed.digest(), token.digest());
    }

    #[test]
    fn fresh_tokens_differ() {
        let first = SecretToken::generate().unwrap();
        let second = SecretToken::generate().unwrap();
        assert!(!first.matches(&second));
        assert_ne!(first.digest(), second.digest());
    }

    #[test]
    fn the_digest_is_blake2s_256() {
        let token = SecretToken::parse(&"00".repeat(32)).unwrap();
        // BLAKE2s-256 of 32 zero bytes.
        let expected = Blake2s256::digest([0_u8; 32]);
        assert_eq!(token.digest().as_bytes().as_slice(), expected.as_slice());
        assert_eq!(
            TokenDigest::from_bytes(*token.digest().as_bytes()),
            token.digest()
        );
    }

    #[test]
    fn malformed_text_is_refused() {
        for bad in [
            String::new(),
            "0".repeat(63),
            "0".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            format!("{}é", "0".repeat(62)),
            format!(" {}", "0".repeat(63)),
        ] {
            assert!(
                matches!(SecretToken::parse(&bad), Err(MalformedToken)),
                "{bad:?}"
            );
        }
        assert_eq!(
            SecretToken::parse(&"0f".repeat(32)).unwrap().encode(),
            "0f".repeat(32)
        );
    }

    #[test]
    fn debug_output_hides_the_bytes() {
        let token = SecretToken::generate().unwrap();
        assert_eq!(format!("{token:?}"), "SecretToken(<redacted>)");
        assert_eq!(format!("{:?}", token.digest()), "TokenDigest(..)");
    }
}
