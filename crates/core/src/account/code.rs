//! Account codes: the identifier a user reads out to the operator so that
//! the operator binds the right portal account to their Mooring user.
//!
//! Only the account's owner sees it (on the account page, after logging
//! in), and the operator binds by code, never by email: open registration
//! proves nothing about who owns an email address.

use std::fmt;

/// Crockford's base32 alphabet in lowercase: no `i`, `l`, `o` or `u`, so a
/// code survives being read aloud.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
/// Symbols in a code: 60 random bits.
const SYMBOLS: usize = 12;
/// Symbols per group in the written form `xxxx-xxxx-xxxx`.
const GROUP: usize = 4;

/// A 60-bit random account code, written `xxxx-xxxx-xxxx` in Crockford's
/// base32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountCode([u8; SYMBOLS]);

/// Why text is not an account code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{text:?} is not an account code (12 symbols written xxxx-xxxx-xxxx)")]
pub struct AccountCodeError {
    /// The refused text.
    pub text: String,
}

impl AccountCode {
    /// A new code from the operating system's random source.
    pub fn generate() -> Result<Self, getrandom::Error> {
        Ok(Self::from_bits(getrandom::u64()?))
    }

    /// The code whose symbols encode the low 60 bits of `bits`, most
    /// significant first.
    #[must_use]
    pub fn from_bits(bits: u64) -> Self {
        let mut symbols = [0_u8; SYMBOLS];
        for (index, symbol) in symbols.iter_mut().enumerate() {
            let shift = 5 * (SYMBOLS - 1 - index);
            // A five-bit value always indexes the 32-symbol alphabet.
            *symbol = ALPHABET[((bits >> shift) & 0x1f) as usize];
        }
        Self(symbols)
    }

    /// Parses a code as a person may type it: either case, with or without
    /// the two hyphens, and with `o` read as `0` and `i` or `l` as `1`
    /// (Crockford's decoding rules).
    pub fn parse(text: &str) -> Result<Self, AccountCodeError> {
        let invalid = || AccountCodeError {
            text: text.to_owned(),
        };
        let bare: Vec<u8> = match text.len() {
            n if n == SYMBOLS => text.bytes().collect(),
            n if n == SYMBOLS + 2 => {
                let bytes = text.as_bytes();
                if bytes.get(GROUP) != Some(&b'-') || bytes.get(2 * GROUP + 1) != Some(&b'-') {
                    return Err(invalid());
                }
                bytes.iter().copied().filter(|&byte| byte != b'-').collect()
            }
            _ => return Err(invalid()),
        };
        let mut symbols = [0_u8; SYMBOLS];
        if bare.len() != SYMBOLS {
            return Err(invalid());
        }
        for (slot, byte) in symbols.iter_mut().zip(bare) {
            *slot = decode_symbol(byte).ok_or_else(invalid)?;
        }
        Ok(Self(symbols))
    }
}

impl fmt::Display for AccountCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, group) in self.0.chunks(GROUP).enumerate() {
            if index > 0 {
                f.write_str("-")?;
            }
            for &symbol in group {
                write!(f, "{}", char::from(symbol))?;
            }
        }
        Ok(())
    }
}

fn decode_symbol(byte: u8) -> Option<u8> {
    let symbol = match byte.to_ascii_lowercase() {
        b'o' => b'0',
        b'i' | b'l' => b'1',
        other => other,
    };
    ALPHABET.contains(&symbol).then_some(symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_written_in_three_groups() {
        let code = AccountCode::from_bits(0);
        assert_eq!(code.to_string(), "0000-0000-0000");
        let code = AccountCode::from_bits(u64::MAX);
        assert_eq!(code.to_string(), "zzzz-zzzz-zzzz");
        // The top four bits are not part of the code.
        assert_eq!(
            AccountCode::from_bits(0xf000_0000_0000_0001),
            AccountCode::from_bits(1)
        );
        assert_eq!(AccountCode::from_bits(1).to_string(), "0000-0000-0001");
        assert_eq!(
            AccountCode::from_bits(0x0123_4567_89ab_cdef).to_string(),
            "28t5-cy4t-qkff"
        );
    }

    #[test]
    fn written_codes_parse_back() {
        for bits in [0, 1, 31, 32, 0x0123_4567_89ab_cdef, u64::MAX] {
            let code = AccountCode::from_bits(bits);
            assert_eq!(AccountCode::parse(&code.to_string()), Ok(code));
            let bare = code.to_string().replace('-', "");
            assert_eq!(AccountCode::parse(&bare), Ok(code));
            assert_eq!(
                AccountCode::parse(&code.to_string().to_uppercase()),
                Ok(code)
            );
        }
    }

    #[test]
    fn confusable_letters_read_as_digits() {
        assert_eq!(
            AccountCode::parse("OOOO-iiii-LLLL"),
            AccountCode::parse("0000-1111-1111")
        );
    }

    #[test]
    fn malformed_codes_are_refused() {
        for bad in [
            "",
            "0000-0000-000",
            "0000-0000-00000",
            "0000000000000",
            "00000-000-0000",
            "0000-00000-000",
            "0000_0000_0000",
            "0000-0000-000u",
            "0000-0000-000!",
            "--0000000000",
            "0000-0000-00é",
            "0000-0000-0-00",
        ] {
            assert_eq!(
                AccountCode::parse(bad),
                Err(AccountCodeError {
                    text: bad.to_owned()
                }),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn generated_codes_differ() {
        let first = AccountCode::generate().unwrap();
        let second = AccountCode::generate().unwrap();
        assert_ne!(first, second);
    }
}
