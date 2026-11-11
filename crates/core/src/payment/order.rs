//! Order ids: the reference a payment carries from the portal to Stripe
//! and back.
//!
//! The portal makes an order before it sends its user to pay, and adds
//! the order's id to the payment page's address (`client_reference_id`);
//! Stripe returns it with the payment. An id is not a secret: paying for
//! somebody's order is a gift to them. It is random so that no two orders
//! share one and none can be guessed in bulk.

use std::fmt;

use blake2::{Blake2s256, Digest as _};

use super::stripe::StripeId;
use crate::auth::RandomError;

/// Crockford's base32 alphabet in lowercase, as account codes use. Stripe
/// accepts letters, digits, dashes and underscores in a reference.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
/// Symbols in an id: 100 random bits.
const SYMBOLS: usize = 20;
/// What is hashed before an invoice's id, so that a renewal's order id is
/// no other use's digest of it.
const RENEWAL: &[u8] = b"mooring-portal renewal order v1\0";

/// A 100-bit random order id, written as 20 symbols of Crockford's base32
/// in lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrderId([u8; SYMBOLS]);

/// Text that is not an order id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not an order id (20 lowercase base32 symbols)")]
pub struct OrderIdError;

impl OrderId {
    /// A new id from the operating system's random source.
    pub fn generate() -> Result<Self, RandomError> {
        let mut bytes = [0_u8; 13];
        getrandom::fill(&mut bytes)?;
        Ok(Self::from_bytes(&bytes))
    }

    /// The id of the order a subscription's renewal makes: the same for
    /// the same invoice, however often Stripe tells of it. Nobody placed
    /// a renewal's order, so its id is worked out from the invoice that
    /// bills it rather than drawn at random; two invoices share one no
    /// more than two random ids do.
    #[must_use]
    pub fn for_renewal(invoice: &StripeId) -> Self {
        let mut hasher = Blake2s256::new();
        hasher.update(RENEWAL);
        hasher.update(invoice.as_str().as_bytes());
        let digest = hasher.finalize();
        let mut bytes = [0_u8; 13];
        bytes.copy_from_slice(&digest[..13]);
        Self::from_bytes(&bytes)
    }

    /// The id whose symbols encode the first 100 bits of `bytes`, five to
    /// a symbol.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 13]) -> Self {
        let mut symbols = [0_u8; SYMBOLS];
        for (index, symbol) in symbols.iter_mut().enumerate() {
            let bit = 5 * index;
            // The five bits that begin `bit` bits in: they lie in one byte
            // or across two.
            let pair = (u16::from(bytes[bit / 8]) << 8)
                | u16::from(bytes.get(bit / 8 + 1).copied().unwrap_or(0));
            let value = (pair >> (11 - bit % 8)) & 0x1f;
            *symbol = ALPHABET[usize::from(value)];
        }
        Self(symbols)
    }

    /// Reads an id exactly as [`fmt::Display`] writes one. Unlike an
    /// account code, nobody types an order id, so no other spelling is
    /// accepted.
    pub fn parse(text: &str) -> Result<Self, OrderIdError> {
        let symbols: [u8; SYMBOLS] = text.as_bytes().try_into().map_err(|_| OrderIdError)?;
        if symbols.iter().all(|symbol| ALPHABET.contains(symbol)) {
            Ok(Self(symbols))
        } else {
            Err(OrderIdError)
        }
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Every symbol is an ASCII letter or digit of the alphabet.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_twenty_symbols_five_bits_each() {
        // All zero bits, all one bits, and one bit pattern that crosses
        // every byte boundary: 00001 00010 00011 ... counts up from 1.
        assert_eq!(
            OrderId::from_bytes(&[0; 13]).as_str(),
            "00000000000000000000"
        );
        assert_eq!(
            OrderId::from_bytes(&[0xff; 13]).as_str(),
            "zzzzzzzzzzzzzzzzzzzz"
        );
        let mut bits = Vec::new();
        for value in 1..=20_u8 {
            for bit in (0..5).rev() {
                bits.push((value >> bit) & 1);
            }
        }
        let mut bytes = [0_u8; 13];
        for (index, bit) in bits.iter().enumerate() {
            bytes[index / 8] |= bit << (7 - index % 8);
        }
        // The last four bits of the thirteenth byte are not read.
        bytes[12] |= 0x0f;
        assert_eq!(
            OrderId::from_bytes(&bytes).to_string(),
            "123456789abcdefghjkm"
        );
    }

    #[test]
    fn ids_are_random_and_read_back() {
        let first = OrderId::generate().unwrap();
        let second = OrderId::generate().unwrap();
        assert_ne!(first, second);
        assert_eq!(OrderId::parse(&first.to_string()), Ok(first));
        assert_eq!(first.as_str().len(), 20);
    }

    #[test]
    fn a_renewals_id_is_worked_out_from_its_invoice() {
        // BLAKE2s-256 of the prefix and the invoice's id, as another
        // program computes it; its first hundred bits, five to a symbol.
        let invoice = |text| StripeId::parse(text).unwrap();
        assert_eq!(
            OrderId::for_renewal(&invoice("in_1SynthCycle")).as_str(),
            "sbc7dxq3cceajeb9ncge"
        );
        assert_eq!(
            OrderId::for_renewal(&invoice("in_1SynthCycle")),
            OrderId::for_renewal(&invoice("in_1SynthCycle"))
        );
        assert_eq!(
            OrderId::for_renewal(&invoice("in_1SynthCycld")).as_str(),
            "15k0jjqr0e1qsbc19nsv"
        );
        assert_eq!(
            OrderId::for_renewal(&invoice("in_1SynthFirst")).as_str(),
            "xs1pa60bdhq2325vgh67"
        );
    }

    #[test]
    fn only_the_written_form_is_an_id() {
        for bad in [
            "",
            "0000000000000000000",
            "000000000000000000000",
            "0000000000000000000O",
            "0000000000000000000i",
            "0000000000000000000l",
            "0000000000000000000u",
            "0000-0000-0000-0000-",
            "0000000000000000000\u{e9}",
        ] {
            assert_eq!(OrderId::parse(bad), Err(OrderIdError), "{bad:?}");
        }
        assert_eq!(
            OrderIdError.to_string(),
            "not an order id (20 lowercase base32 symbols)"
        );
    }
}
