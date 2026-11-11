//! A portal that sells: the demonstration's catalogue, a signing secret,
//! and Stripe's deliveries, signed as Stripe signs them.

use std::fs;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use hmac::{Hmac, KeyInit as _, Mac as _};
use portal_core::account::AccountCode;
use portal_core::auth::SecretToken;
use portal_core::payment::Catalogue;
use portal_core::payment::stripe::SigningSecret;
use portal_core::time::UnixTime;
use sha2::Sha256;

use super::support::{Browser, DatabaseFile, NOW, Reply, TestApp};
use crate::config::{ClientIpSource, Payments};

/// The secret the test portal believes deliveries under.
pub(crate) const SECRET: &str = "whsec_test_portal_0123456789abcdef";
/// The order every event among the fixtures names.
pub(crate) const FIXTURE_ORDER: &str = "0123456789abcdefghjk";
/// An address deliveries come from.
const STRIPE: [u8; 4] = [192, 0, 2, 44];

fn fixture_file(sub: &str) -> String {
    let file: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../../contract/fixtures", sub]
        .iter()
        .collect();
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The demonstration's catalogue: a month, a renewing month and a pack.
pub(crate) fn catalogue() -> Catalogue {
    Catalogue::parse(&fixture_file("payments/demo.json")).unwrap()
}

/// The demonstration's catalogue as it would be written for real
/// payments.
pub(crate) fn live_catalogue() -> Catalogue {
    let written = fixture_file("payments/demo.json");
    let live = written.replace("\"livemode\": false", "\"livemode\": true");
    assert_ne!(live, written);
    Catalogue::parse(&live).unwrap()
}

/// What sells `catalogue` and believes deliveries signed under
/// [`SECRET`].
pub(crate) fn payments_on(catalogue: Catalogue) -> Payments {
    Payments::new(catalogue, vec![SigningSecret::parse(SECRET).unwrap()]).unwrap()
}

/// `event` as Stripe would send it for a payment made with real money.
pub(crate) fn live(event: &str) -> String {
    let live = event.replace(
        "\"livemode\": false,\n  \"pending_webhooks\"",
        "\"livemode\": true,\n  \"pending_webhooks\"",
    );
    assert_ne!(live, event);
    live
}

/// The fixture event `name`, naming `order` where it names an order.
pub(crate) fn event(name: &str, order: &str) -> String {
    fixture_file(&format!("stripe-events/{name}.json")).replace(FIXTURE_ORDER, order)
}

/// The `Stripe-Signature` of `body`, signed at `at` under `secret`.
pub(crate) fn signature(secret: &str, at: u64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{at}.{body}").as_bytes());
    let digest: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("t={at},v1={digest}")
}

impl TestApp {
    /// A portal that sells the demonstration's catalogue and believes
    /// deliveries signed under [`SECRET`].
    pub(crate) fn shop() -> Self {
        Self::shop_with(&[SECRET], ClientIpSource::Peer)
    }

    /// [`Self::shop`], believing deliveries under any of `secrets`.
    pub(crate) fn shop_with(secrets: &[&str], client_ip: ClientIpSource) -> Self {
        let secrets = secrets
            .iter()
            .map(|secret| SigningSecret::parse(secret).unwrap())
            .collect();
        Self::selling(Payments::new(catalogue(), secrets).unwrap(), client_ip)
    }

    /// [`Self::shop`] over a database file, which the test may also open.
    pub(crate) fn shop_on_disk() -> (Self, DatabaseFile) {
        Self::on_disk_with(Some(payments_on(catalogue())))
    }

    /// A portal that sells the demonstration's catalogue for real money.
    pub(crate) fn live_shop() -> Self {
        Self::selling(payments_on(live_catalogue()), ClientIpSource::Peer)
    }

    /// A delivery of `body` with the `Stripe-Signature` `signature`, if
    /// any.
    pub(crate) async fn deliver_signed(&self, body: &str, signature: Option<&str>) -> Reply {
        let mut request = Browser::new(STRIPE).request("POST", "/stripe/webhook");
        if let Some(signature) = signature {
            request = request.header("stripe-signature", signature);
        }
        self.send(request.body(Body::from(body.to_owned())).unwrap())
            .await
    }

    /// A delivery of `body` signed now under [`SECRET`].
    pub(crate) async fn deliver(&self, body: &str) -> Reply {
        let now = self.state.now().unwrap().as_secs();
        self.deliver_signed(body, Some(&signature(SECRET, now, body)))
            .await
    }

    /// Delivers each of the fixture events `names`, naming `order`; every
    /// one must be accepted.
    pub(crate) async fn deliver_events(&self, names: &[&str], order: &str) {
        for name in names {
            let reply = self.deliver(&event(name, order)).await;
            assert_eq!(reply.status, StatusCode::OK, "{name}: {}", reply.body);
        }
    }

    /// Makes a token for Mooring to pull with, and gives it.
    pub(crate) async fn pull_token(&self) -> String {
        let token = SecretToken::generate().unwrap();
        let digest = token.digest();
        let now = UnixTime::from_secs(NOW).unwrap();
        self.store(move |store| store.set_pull_token(&digest, now))
            .await
            .unwrap();
        token.encode()
    }

    /// A pull of the entitlements at `path`, bearing `authorization` if
    /// any.
    pub(crate) async fn pull(&self, path: &str, authorization: Option<&str>) -> Reply {
        let mut request = Browser::new([192, 0, 2, 99]).request("GET", path);
        if let Some(authorization) = authorization {
            request = request.header(AUTHORIZATION, authorization);
        }
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// A signed-up account bound to `user`, in a browser that stays
    /// logged in.
    pub(crate) async fn customer(&self, email: &str, user: &str) -> (Browser, AccountCode) {
        let (browser, code) = self.signed_up(email).await;
        self.bind(code, user).await;
        (browser, code)
    }
}

impl Browser {
    /// Orders `plan` and gives the new order's id.
    pub(crate) async fn order(&mut self, app: &TestApp, plan: &str) -> String {
        let reply = self
            .post(app, "/subscription/orders", &[("plan", plan)])
            .await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);
        let id = reply
            .location()
            .strip_prefix("/subscription/orders/")
            .unwrap_or_else(|| panic!("{}", reply.location()));
        assert_eq!(id.len(), 20, "{id}");
        id.to_owned()
    }

    /// The page of the order `id`.
    pub(crate) async fn order_page(&mut self, app: &TestApp, id: &str) -> Reply {
        self.get(app, &format!("/subscription/orders/{id}")).await
    }
}
