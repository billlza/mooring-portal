//! Login email addresses.

use std::fmt;

use blake2::{Blake2s256, Digest as _};

/// The longest address accepted, in bytes (RFC 5321's path limit less the
/// angle brackets).
const MAX_LEN: usize = 254;
/// The longest local part accepted, in bytes (RFC 5321).
const MAX_LOCAL_LEN: usize = 64;
/// The longest host-name label accepted, in bytes (RFC 1035).
const MAX_LABEL_LEN: usize = 63;
/// The non-alphanumeric characters of an RFC 5322 dot-atom.
const ATOM_SPECIALS: &[u8] = b"!#$%&'*+-/=?^_`{|}~";

/// A login email address, normalised to lowercase ASCII.
///
/// The portal sends no mail in v1, so nothing proves that the user receives
/// mail at the address: it is a login name the user remembers. Validation
/// is deliberately simple and strict: ASCII only, exactly one `@`, a
/// dot-atom local part and a dotted host name whose last label is not all
/// digits. Its `Debug` output is masked, so an address never reaches a log
/// in full.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Email(String);

/// Why an email address was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EmailError {
    /// Nothing but white space.
    #[error("the email address is empty")]
    Empty,
    /// Longer than 254 bytes.
    #[error("the email address is longer than {MAX_LEN} bytes")]
    TooLong,
    /// A character outside ASCII.
    #[error("the email address contains characters other than ASCII")]
    NotAscii,
    /// No `@`, or more than one.
    #[error("the email address needs exactly one @")]
    At,
    /// The part before `@` is not a dot-atom of at most 64 bytes.
    #[error("the part before @ is not valid")]
    LocalPart,
    /// The part after `@` is not a dotted host name.
    #[error("the part after @ is not a valid host name")]
    Domain,
}

impl Email {
    /// Validates and normalises `input`; surrounding white space is ignored.
    pub fn parse(input: &str) -> Result<Self, EmailError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(EmailError::Empty);
        }
        if trimmed.len() > MAX_LEN {
            return Err(EmailError::TooLong);
        }
        if !trimmed.is_ascii() {
            return Err(EmailError::NotAscii);
        }
        let normalised = trimmed.to_ascii_lowercase();
        let (local, domain) = normalised.split_once('@').ok_or(EmailError::At)?;
        if domain.contains('@') {
            return Err(EmailError::At);
        }
        if !is_dot_atom(local) {
            return Err(EmailError::LocalPart);
        }
        if !is_host_name(domain) {
            return Err(EmailError::Domain);
        }
        Ok(Self(normalised))
    }

    /// The normalised address.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before `@`.
    #[must_use]
    pub fn local_part(&self) -> &str {
        self.0.split_once('@').map_or("", |(local, _)| local)
    }

    /// The address with all but the first character of its local part
    /// hidden, for logs: `a***@example.com`.
    #[must_use]
    pub fn masked(&self) -> String {
        let (local, domain) = self.0.split_once('@').unwrap_or((&self.0, ""));
        let first = local.chars().next().map_or(String::new(), String::from);
        format!("{first}***@{domain}")
    }
}

/// What the ban list keeps of a banned address: its BLAKE2s-256 digest, of
/// the address as [`Email::parse`] normalises it.
///
/// The list must outlive the account it came from, and a ban implies an
/// accusation, so the list does not keep the addresses themselves: a
/// leaked copy of the database lists no banned address. Anyone who holds
/// a candidate address can still test it, so this hides the list, not a
/// given address.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EmailDigest([u8; 32]);

impl EmailDigest {
    /// The digest's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for EmailDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EmailDigest(..)")
    }
}

impl Email {
    /// The digest the ban list keeps of this address. Two addresses share
    /// it only when they normalise alike: surrounding white space and
    /// letter case are ignored, nothing else. Dots and `+` suffixes in the
    /// local part are kept, and no two domains are taken for one, because
    /// those rules hold for some mail services only, and elsewhere they
    /// would make two people one.
    #[must_use]
    pub fn digest(&self) -> EmailDigest {
        EmailDigest(Blake2s256::digest(self.0.as_bytes()).into())
    }
}

impl fmt::Debug for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Email").field(&self.masked()).finish()
    }
}

fn is_dot_atom(local: &str) -> bool {
    !local.is_empty()
        && local.len() <= MAX_LOCAL_LEN
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || ATOM_SPECIALS.contains(&byte)
        })
}

fn is_host_name(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    let top_is_numeric = labels
        .last()
        .is_some_and(|top| top.bytes().all(|byte| byte.is_ascii_digit()));
    labels.len() >= 2 && labels.iter().all(|label| is_label(label)) && !top_is_numeric
}

fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ban_digest_ignores_only_case_and_white_space() {
        let digest = |text: &str| Email::parse(text).unwrap().digest();
        let banned = digest("Li.Lei+Portal@QQ.com");
        assert_eq!(banned, digest("  li.lei+portal@qq.COM \n"));
        assert_eq!(
            banned.as_bytes(),
            &<[u8; 32]>::from(Blake2s256::digest(b"li.lei+portal@qq.com"))
        );
        // Other people's addresses, under rules some services apply.
        for other in [
            "lilei+portal@qq.com",
            "li.lei@qq.com",
            "li.lei+other@qq.com",
            "li.lei+portal@foxmail.com",
        ] {
            assert_ne!(banned, digest(other), "{other}");
        }
        assert_eq!(format!("{banned:?}"), "EmailDigest(..)");
    }

    #[test]
    fn addresses_are_trimmed_and_lowercased() {
        let email = Email::parse("  Li.Lei+Portal@QQ.com \n").unwrap();
        assert_eq!(email.as_str(), "li.lei+portal@qq.com");
        assert_eq!(email.local_part(), "li.lei+portal");
        assert_eq!(email, Email::parse("li.lei+portal@qq.com").unwrap());
    }

    #[test]
    fn masked_forms_hide_the_local_part() {
        let email = Email::parse("lilei@163.com").unwrap();
        assert_eq!(email.masked(), "l***@163.com");
        assert_eq!(format!("{email:?}"), "Email(\"l***@163.com\")");
        assert!(!format!("{email:?}").contains("lilei"));
    }

    #[test]
    fn valid_addresses() {
        for good in [
            "a@b.cn",
            "x_y-z@mail.example.co.uk",
            "o'neil@example.org",
            "1234567@qq.com",
            "user@xn--fiqs8s.xn--fiqs8s",
            &format!("{}@example.com", "a".repeat(64)),
        ] {
            assert!(Email::parse(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn invalid_addresses_say_why() {
        let long_domain = format!("a@{}.com", vec!["b".repeat(60); 5].join("."));
        for (bad, error) in [
            ("", EmailError::Empty),
            ("   ", EmailError::Empty),
            (long_domain.as_str(), EmailError::TooLong),
            ("李雷@qq.com", EmailError::NotAscii),
            ("lilei.qq.com", EmailError::At),
            ("a@b@qq.com", EmailError::At),
            ("@qq.com", EmailError::LocalPart),
            (".a@qq.com", EmailError::LocalPart),
            ("a.@qq.com", EmailError::LocalPart),
            ("a..b@qq.com", EmailError::LocalPart),
            ("a b@qq.com", EmailError::LocalPart),
            ("a\"b@qq.com", EmailError::LocalPart),
            ("a@", EmailError::Domain),
            ("a@qq", EmailError::Domain),
            ("a@qq.", EmailError::Domain),
            ("a@.qq.com", EmailError::Domain),
            ("a@qq..com", EmailError::Domain),
            ("a@-qq.com", EmailError::Domain),
            ("a@qq-.com", EmailError::Domain),
            ("a@q_q.com", EmailError::Domain),
            ("a@192.168.0.1", EmailError::Domain),
            ("a@[192.168.0.1]", EmailError::Domain),
        ] {
            assert_eq!(Email::parse(bad), Err(error), "{bad}");
        }
        let long_local = format!("{}@example.com", "a".repeat(65));
        assert_eq!(Email::parse(&long_local), Err(EmailError::LocalPart));
        let long_label = format!("a@{}.com", "b".repeat(64));
        assert_eq!(Email::parse(&long_label), Err(EmailError::Domain));
    }
}
