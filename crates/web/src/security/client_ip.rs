//! The client network a request comes from, for rate limits.
//!
//! IPv6 clients are limited per /48. A /64 is the smallest block an ISP
//! assigns, but tunnel brokers hand out a whole /48 (65,536 /64s) for free,
//! so limiting per /64 would let one person fill a limiter with fresh keys
//! (see `portal_core::rate_limit`). The cost is that clients who share a
//! /48, such as neighbours at a mobile carrier, share its allowance; with a
//! handful of users that is rare and the allowance is ample. Addresses are
//! kept in memory only, never logged or stored.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use axum::http::HeaderMap;

use crate::config::ClientIpSource;
use crate::error::AppError;

/// The network a client is limited as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Network {
    /// One IPv4 address.
    V4([u8; 4]),
    /// An IPv6 /48.
    V6([u8; 6]),
}

impl Network {
    pub(crate) fn of(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(v4) => Self::V4(v4.octets()),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => Self::V4(v4.octets()),
                None => Self::V6(prefix48(v6)),
            },
        }
    }
}

fn prefix48(address: Ipv6Addr) -> [u8; 6] {
    let [a, b, c, d, e, f, ..] = address.octets();
    [a, b, c, d, e, f]
}

/// The client network of a request whose TCP peer is `peer`.
pub(crate) fn client_network(
    source: ClientIpSource,
    peer: SocketAddr,
    headers: &HeaderMap,
) -> Result<Network, AppError> {
    match source {
        ClientIpSource::Peer => Ok(Network::of(peer.ip())),
        ClientIpSource::ForwardedFor => {
            if !peer.ip().is_loopback() {
                return Err(AppError::internal_message(format!(
                    "a request came from a non-loopback peer although the portal is \
                     configured to sit behind a local reverse proxy ({})",
                    if peer.is_ipv4() { "IPv4" } else { "IPv6" }
                )));
            }
            let last = headers
                .get_all("x-forwarded-for")
                .iter()
                .next_back()
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.rsplit(',').next())
                .map(str::trim)
                .ok_or_else(|| {
                    AppError::internal_message(
                        "the reverse proxy did not send X-Forwarded-For; configure it to",
                    )
                })?;
            let address: IpAddr = last.parse().map_err(|_| {
                AppError::internal_message(
                    "the reverse proxy sent an X-Forwarded-For that is not an address",
                )
            })?;
            Ok(Network::of(address))
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn peer(text: &str) -> SocketAddr {
        text.parse().unwrap()
    }

    #[test]
    fn ipv6_clients_are_grouped_by_48() {
        let a = Network::of("2001:db8:1:2:aaaa::1".parse().unwrap());
        // Another /64 of the same /48, as a tunnel broker hands out freely.
        let b = Network::of("2001:db8:1:ffff:bbbb::9".parse().unwrap());
        let c = Network::of("2001:db8:2::1".parse().unwrap());
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(
            Network::of("::ffff:203.0.113.7".parse().unwrap()),
            Network::of("203.0.113.7".parse().unwrap())
        );
    }

    #[test]
    fn the_peer_is_the_client_by_default() {
        let network = client_network(
            ClientIpSource::Peer,
            peer("203.0.113.7:5000"),
            &HeaderMap::new(),
        )
        .unwrap();
        assert_eq!(network, Network::V4([203, 0, 113, 7]));
    }

    #[test]
    fn behind_a_proxy_the_last_forwarded_address_counts() {
        let mut headers = HeaderMap::new();
        headers.append("x-forwarded-for", HeaderValue::from_static("198.51.100.1"));
        headers.append(
            "x-forwarded-for",
            HeaderValue::from_static("10.0.0.1, 203.0.113.9"),
        );
        let network = client_network(
            ClientIpSource::ForwardedFor,
            peer("127.0.0.1:9000"),
            &headers,
        )
        .unwrap();
        assert_eq!(network, Network::V4([203, 0, 113, 9]));
        let network =
            client_network(ClientIpSource::ForwardedFor, peer("[::1]:9000"), &headers).unwrap();
        assert_eq!(network, Network::V4([203, 0, 113, 9]));
    }

    #[test]
    fn a_misconfigured_proxy_is_an_error() {
        let mut headers = HeaderMap::new();
        for (peer_text, header) in [
            ("203.0.113.1:9000", Some("198.51.100.1")),
            ("[2001:db8::1]:9000", Some("198.51.100.1")),
            ("127.0.0.1:9000", None),
            ("127.0.0.1:9000", Some("not an address")),
        ] {
            headers.clear();
            if let Some(value) = header {
                headers.insert("x-forwarded-for", HeaderValue::from_static(value));
            }
            assert!(matches!(
                client_network(ClientIpSource::ForwardedFor, peer(peer_text), &headers),
                Err(AppError::Internal(_))
            ));
        }
    }
}
