//! A portal in memory and simulated browsers with their own cookies.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, Request, StatusCode};
use portal_core::account::AccountCode;
use portal_core::auth::HashCost;
use portal_core::snapshot::{MooringUserId, Snapshot};
use portal_core::time::{ManualClock, UnixTime};
use portal_store::{OpenMode, Store};
use tower::ServiceExt as _;

use crate::config::{ClientIpSource, PublicOrigin, WebConfig};
use crate::state::AppState;

/// The origin the test portal is configured with.
pub(crate) const ORIGIN_URL: &str = "https://portal.example";
/// 2026-11-05T12:00:00Z.
pub(crate) const NOW: u64 = 1_793_880_000;
/// A password every test account uses.
pub(crate) const PASSWORD: &str = "correct horse battery";

/// A portal over an in-memory database with a clock the test controls.
/// Clones share the portal, as concurrent requests do.
#[derive(Clone)]
pub(crate) struct TestApp {
    pub(crate) state: AppState,
    pub(crate) clock: Arc<ManualClock>,
    router: Router,
}

/// A database file in a directory removed with it.
pub(crate) struct DatabaseFile {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl DatabaseFile {
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// A response, read whole.
#[derive(Debug)]
pub(crate) struct Reply {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: String,
}

impl Reply {
    /// Where a redirect points.
    pub(crate) fn location(&self) -> &str {
        self.headers
            .get(LOCATION)
            .map(|value| value.to_str().unwrap())
            .unwrap_or_default()
    }
}

/// A browser: a client address and a cookie jar.
#[derive(Debug, Clone)]
pub(crate) struct Browser {
    pub(crate) peer: SocketAddr,
    pub(crate) cookies: BTreeMap<String, String>,
}

impl TestApp {
    pub(crate) fn new() -> Self {
        Self::with(ClientIpSource::Peer, 100)
    }

    pub(crate) fn with(client_ip: ClientIpSource, max_accounts: u32) -> Self {
        Self::over(Store::open_in_memory().unwrap(), client_ip, max_accounts)
    }

    /// A portal over a database file, which the test may also open.
    pub(crate) fn on_disk() -> (Self, DatabaseFile) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portal.db");
        let app = Self::over(
            Store::open(&path, OpenMode::CreateIfMissing).unwrap(),
            ClientIpSource::Peer,
            100,
        );
        (app, DatabaseFile { _dir: dir, path })
    }

    fn over(store: Store, client_ip: ClientIpSource, max_accounts: u32) -> Self {
        let clock = Arc::new(ManualClock::new(UnixTime::from_secs(NOW).unwrap()));
        let config = WebConfig {
            origin: PublicOrigin::parse(ORIGIN_URL).unwrap(),
            client_ip,
            max_accounts,
        };
        let state =
            AppState::new(store, config, HashCost::MINIMUM_FOR_TESTS, clock.clone()).unwrap();
        Self {
            router: crate::router(state.clone()),
            state,
            clock,
        }
    }

    pub(crate) async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        Reply {
            status,
            headers,
            body: String::from_utf8(body.to_vec()).unwrap(),
        }
    }

    /// Runs `work` on the store.
    pub(crate) async fn store<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Store) -> R + Send + 'static,
    ) -> R {
        self.state.database().run(work).await.unwrap()
    }

    /// Binds the account with `code` to `user`.
    pub(crate) async fn bind(&self, code: AccountCode, user: &str) {
        let user = MooringUserId::parse(user).unwrap();
        self.store(move |store| store.bind(&code, &user, UnixTime::from_secs(NOW).unwrap()))
            .await
            .unwrap();
    }

    /// Imports the snapshot document `json`.
    pub(crate) async fn import(&self, json: String) {
        let snapshot = Snapshot::parse(json.as_bytes()).unwrap();
        let now = UnixTime::from_secs(NOW).unwrap();
        self.store(move |store| store.import(&snapshot, now))
            .await
            .unwrap();
    }

    /// Registers `email` from a fresh browser, which stays logged in.
    pub(crate) async fn signed_up(&self, email: &str) -> (Browser, AccountCode) {
        let mut browser = Browser::new([198, 51, 100, 7]);
        browser.get(self, "/register").await;
        let reply = browser
            .post(
                self,
                "/register",
                &[
                    ("email", email),
                    ("password", PASSWORD),
                    ("confirm", PASSWORD),
                ],
            )
            .await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);
        let page = browser.get(self, "/account").await;
        let code = page
            .body
            .split("<p class=\"code\">")
            .nth(1)
            .and_then(|rest| rest.split("</p>").next())
            .unwrap();
        (browser, AccountCode::parse(code).unwrap())
    }
}

impl Browser {
    pub(crate) fn new(address: [u8; 4]) -> Self {
        Self {
            peer: SocketAddr::from((address, 40_000)),
            cookies: BTreeMap::new(),
        }
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn remember(&mut self, reply: &Reply) {
        for value in reply.headers.get_all(SET_COOKIE) {
            let text = value.to_str().unwrap();
            let (pair, attributes) = text.split_once(';').unwrap_or((text, ""));
            let (name, value) = pair.split_once('=').unwrap();
            if attributes.contains("Max-Age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.to_owned(), value.to_owned());
            }
        }
    }

    /// A request carrying this browser's cookies and address.
    pub(crate) fn request(&self, method: &str, path: &str) -> axum::http::request::Builder {
        let mut builder = Request::builder().method(method).uri(path);
        if !self.cookies.is_empty() {
            builder = builder.header(COOKIE, self.cookie_header());
        }
        builder.extension(ConnectInfo(self.peer))
    }

    pub(crate) async fn get(&mut self, app: &TestApp, path: &str) -> Reply {
        let reply = app
            .send(self.request("GET", path).body(Body::empty()).unwrap())
            .await;
        self.remember(&reply);
        reply
    }

    /// Posts `fields` as a form from this site, with the CSRF token of the
    /// browser's cookie.
    pub(crate) async fn post(
        &mut self,
        app: &TestApp,
        path: &str,
        fields: &[(&str, &str)],
    ) -> Reply {
        let token = self.cookies.get("__Host-csrf").cloned().unwrap_or_default();
        let mut all = vec![("csrf", token.as_str())];
        all.extend_from_slice(fields);
        let reply = app
            .send(
                self.request("POST", path)
                    .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(ORIGIN, ORIGIN_URL)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::from(serde_urlencoded::to_string(&all).unwrap()))
                    .unwrap(),
            )
            .await;
        self.remember(&reply);
        reply
    }

    /// Logs in with `email` and the test password.
    pub(crate) async fn log_in(&mut self, app: &TestApp, email: &str) -> Reply {
        self.get(app, "/login").await;
        self.post(app, "/login", &[("email", email), ("password", PASSWORD)])
            .await
    }
}

/// A one-user snapshot document for `user`, generated at [`NOW`].
pub(crate) fn snapshot_json(user: &str, served: Option<u64>) -> String {
    let served = served.map_or("null".to_owned(), |bytes| bytes.to_string());
    format!(
        r#"{{"format":"mooring-portal-snapshot","version":1,"generated_at":{NOW},
        "cycle":{{"start":"2026-11-01","next_start":"2026-12-01"}},"counted_through":{NOW},
        "users":[{{"user":"{user}","status":"active","cycle_served_bytes":{served},
        "reality_downlink_counted_at_close":true,
        "quota":{{"cycle_served_bytes":40000000000,"warn_percent":80,"action":"suspend"}},
        "expires_at":null}}]}}"#
    )
}
