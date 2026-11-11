//! The server's configuration as the web layer needs it.

use std::fmt;
use std::sync::Arc;

use portal_core::payment::stripe::SigningSecret;
use portal_core::payment::{Catalogue, OrderTerms};

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
    /// What the portal sells and how it knows Stripe's word; without it
    /// the portal sells nothing, and none of the payment addresses exist.
    pub payments: Option<Payments>,
}

/// What the portal needs to take orders: the operator's catalogue, and
/// the secrets Stripe signs its deliveries with. The portal holds no key
/// of Stripe's that could charge, refund or look anything up.
#[derive(Debug, Clone)]
pub struct Payments {
    catalogue: Catalogue,
    on_sale: Arc<[OrderTerms]>,
    secrets: Vec<SigningSecret>,
}

/// The signing secrets given are not one or two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("give one signing secret, or two while one is being rolled, not {0}")]
pub struct SecretCount(pub usize);

impl Payments {
    /// Payments on `catalogue`, with Stripe's deliveries believed under
    /// any of `secrets`: the endpoint's one, or its old and its new while
    /// the operator rolls it.
    pub fn new(catalogue: Catalogue, secrets: Vec<SigningSecret>) -> Result<Self, SecretCount> {
        if matches!(secrets.len(), 1 | 2) {
            Ok(Self {
                on_sale: catalogue.plans().iter().map(OrderTerms::of).collect(),
                catalogue,
                secrets,
            })
        } else {
            Err(SecretCount(secrets.len()))
        }
    }

    /// What the portal sells.
    pub(crate) const fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    /// The terms of every plan on sale: a payment for an order placed on
    /// other terms is not granted unseen.
    pub(crate) fn on_sale(&self) -> Arc<[OrderTerms]> {
        Arc::clone(&self.on_sale)
    }

    /// The secrets Stripe's deliveries are believed under.
    pub(crate) fn secrets(&self) -> &[SigningSecret] {
        &self.secrets
    }
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
    fn payments_take_one_signing_secret_or_two() {
        let catalogue = || {
            Catalogue::parse(
                r#"{"format":"mooring-portal-payments","version":1,"livemode":false,
                "currency":"gbp","plans":[{"id":"month","kind":"month","days":30,
                "traffic_gb":100,"amount":800,"payment_link":"https://buy.stripe.com/test_a",
                "payment_link_id":"plink_1a"}],"manage_subscription_url":null}"#,
            )
            .unwrap()
        };
        let secret =
            |n: u8| SigningSecret::parse(&format!("whsec_{}", n.to_string().repeat(20))).unwrap();
        for count in [1_u8, 2] {
            let payments = Payments::new(catalogue(), (0..count).map(secret).collect()).unwrap();
            assert_eq!(payments.secrets().len(), usize::from(count));
            assert_eq!(payments.catalogue().plans().len(), 1);
            assert_eq!(
                *payments.on_sale(),
                [OrderTerms::of(&payments.catalogue().plans()[0])]
            );
            // A secret is not printed with the rest.
            assert!(!format!("{payments:?}").contains("whsec_"));
        }
        for count in [0_u8, 3] {
            let refused = Payments::new(catalogue(), (0..count).map(secret).collect());
            assert_eq!(refused.err(), Some(SecretCount(usize::from(count))));
        }
        assert_eq!(
            SecretCount(0).to_string(),
            "give one signing secret, or two while one is being rolled, not 0"
        );
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
