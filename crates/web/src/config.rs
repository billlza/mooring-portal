//! The server's configuration as the web layer needs it.

use std::fmt;

/// The longest a login session lasts.
pub const SESSION_TTL_SECS: u64 = 30 * 86_400;

/// How the portal is configured.
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// The origin browsers reach the portal at; state-changing requests
    /// from any other origin are refused.
    pub origin: PublicOrigin,
    /// Where a request's client address comes from.
    pub client_ip: ClientIpSource,
    /// Registration closes once the portal holds this many accounts.
    pub max_accounts: u32,
}

/// Where the client address used for rate limits comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIpSource {
    /// The TCP peer: the portal faces browsers directly.
    Peer,
    /// The last address of `X-Forwarded-For`, appended by a reverse proxy
    /// on the same host; requests from any other peer are refused.
    ForwardedFor,
}

/// A browser-facing origin: `https://host[:port]`, or `http://` for a
/// loopback host during development. Default ports are dropped, as
/// browsers drop them from the `Origin` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicOrigin(String);

/// Text that is not an acceptable origin.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OriginError {
    /// Not `scheme://host[:port]` with nothing after.
    #[error("{0:?} is not an origin: write scheme://host[:port] with no path")]
    Malformed(String),
    /// Plain HTTP for a host other than this machine: `__Host-` cookies
    /// need HTTPS there.
    #[error("{0:?} uses http for a host other than localhost; use https")]
    InsecureScheme(String),
}

impl PublicOrigin {
    /// Validates and normalises `text`.
    pub fn parse(text: &str) -> Result<Self, OriginError> {
        let malformed = || OriginError::Malformed(text.to_owned());
        let lowered = text.to_ascii_lowercase();
        let (scheme, rest) = lowered.split_once("://").ok_or_else(malformed)?;
        let default_port = match scheme {
            "https" => "443",
            "http" => "80",
            _ => return Err(malformed()),
        };
        if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ']) {
            return Err(malformed());
        }
        let (host, port) = split_port(rest).ok_or_else(malformed)?;
        if !valid_host(host) {
            return Err(malformed());
        }
        if scheme == "http" && !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            return Err(OriginError::InsecureScheme(text.to_owned()));
        }
        Ok(Self(match port {
            Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
            _ => format!("{scheme}://{host}"),
        }))
    }

    /// The origin as browsers write it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PublicOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Splits `host[:port]`, where an IPv6 host is bracketed; the port, if
/// given, is 1..=65535 without leading zeros.
fn split_port(authority: &str) -> Option<(&str, Option<&str>)> {
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let (host, after) = authority.split_at(end + 1);
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return None,
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if let Some(port) = port {
        let valid = !port.starts_with('0') && port.parse::<u16>().is_ok_and(|port| port > 0);
        if !valid {
            return None;
        }
    }
    Some((host, port))
}

fn valid_host(host: &str) -> bool {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return inner.parse::<std::net::Ipv6Addr>().is_ok();
    }
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_normalised() {
        for (text, origin) in [
            ("https://portal.example", "https://portal.example"),
            ("HTTPS://Portal.Example:443", "https://portal.example"),
            ("https://portal.example:8443", "https://portal.example:8443"),
            ("http://localhost:8080", "http://localhost:8080"),
            ("http://127.0.0.1:80", "http://127.0.0.1"),
            ("http://[::1]:8080", "http://[::1]:8080"),
            ("https://[2001:db8::1]", "https://[2001:db8::1]"),
        ] {
            assert_eq!(
                PublicOrigin::parse(text).unwrap().as_str(),
                origin,
                "{text}"
            );
        }
        assert_eq!(
            PublicOrigin::parse("https://a.example")
                .unwrap()
                .to_string(),
            "https://a.example"
        );
    }

    #[test]
    fn malformed_origins_are_refused() {
        for bad in [
            "",
            "portal.example",
            "ftp://portal.example",
            "https://",
            "https://portal.example/",
            "https://portal.example/path",
            "https://portal.example?x",
            "https://user@portal.example",
            "https://portal.example:0",
            "https://portal.example:080",
            "https://portal.example:65536",
            "https://portal.example:",
            "https://portal..example",
            "https://-portal.example",
            "https://[::1",
            "https://[nothost]",
            "https://[::1]x",
            "https://por tal.example",
        ] {
            assert_eq!(
                PublicOrigin::parse(bad),
                Err(OriginError::Malformed(bad.to_owned())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn plain_http_is_only_for_this_machine() {
        assert_eq!(
            PublicOrigin::parse("http://portal.example"),
            Err(OriginError::InsecureScheme(
                "http://portal.example".to_owned()
            ))
        );
        assert_eq!(SESSION_TTL_SECS, 2_592_000);
    }
}
