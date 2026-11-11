//! What Stripe delivers and what Mooring pulls.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, RETRY_AFTER, SET_COOKIE, WWW_AUTHENTICATE};

use super::logs::{capture_logs, logged};
use super::paying::{FIXTURE_ORDER, SECRET, event, live, signature};
use super::support::{Browser, NOW, Reply, TestApp};
use crate::config::ClientIpSource;
use crate::security::limits::{PULLS_PER_NETWORK, WEBHOOKS_PER_NETWORK};

const FEED: &str = "/api/mooring/entitlements";
const OTHER_SECRET: &str = "whsec_test_other_fedcba9876543210";

/// What a machine is told when it is refused: a line or two of plain
/// text, and no cookie.
fn refused(reply: &Reply, status: StatusCode) {
    assert_eq!(reply.status, status, "{}", reply.body);
    assert_eq!(
        reply.headers[CONTENT_TYPE], "text/plain; charset=utf-8",
        "{}",
        reply.body
    );
    assert!(reply.headers.get(SET_COOKIE).is_none());
    assert!(!reply.body.contains('<'), "{}", reply.body);
}

/// How many orders have been paid.
async fn orders_recorded(app: &TestApp) -> usize {
    app.store(|store| store.recent_orders(100))
        .await
        .unwrap()
        .into_iter()
        .filter(|listed| listed.order.paid_at.is_some())
        .count()
}

#[tokio::test]
async fn a_delivery_signed_by_stripe_is_recorded_and_answered_with_nothing() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    let checkout = event("checkout-payment", &id);
    let accepted = app.deliver(&checkout).await;
    assert_eq!(accepted.status, StatusCode::OK);
    assert_eq!(accepted.body, "");
    assert!(accepted.headers.get(SET_COOKIE).is_none());
    assert_eq!(orders_recorded(&app).await, 1);
    // Delivered again, it is accepted again and changes nothing.
    assert_eq!(app.deliver(&checkout).await.status, StatusCode::OK);
    // An event the portal does not act on is accepted too, whatever
    // release it is rendered for.
    let unused = event("unused-type", &id).replace("2026-10-28.endive", "2019-12-03");
    assert_eq!(app.deliver(&unused).await.status, StatusCode::OK);
    app.deliver_events(&["charge-payment"], &id).await;
    let granted = app
        .store(|store| store.pull_state())
        .await
        .unwrap()
        .last_seq;
    assert_eq!(granted, 1);
}

#[tokio::test]
async fn a_delivery_is_believed_only_under_the_portals_secret_and_in_time() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    let body = event("checkout-payment", &id);
    let signed = |secret: &str, at: u64, body: &str| signature(secret, at, body);
    // Not signed at all; signed under another secret; a signature of
    // another body; a header that is none.
    let good = signed(SECRET, NOW, &body);
    for (what, header) in [
        ("unsigned", None),
        ("another secret", Some(signed(OTHER_SECRET, NOW, &body))),
        (
            "another body",
            Some(signed(SECRET, NOW, &format!("{body} "))),
        ),
        (
            "another time",
            Some(good.replace(&format!("t={NOW}"), &format!("t={}", NOW + 1))),
        ),
        ("no signature", Some(format!("t={NOW}"))),
        ("no time", Some(good.replace(&format!("t={NOW},"), ""))),
        ("not a header", Some("signed, honest".to_owned())),
        ("empty", Some(String::new())),
    ] {
        let reply = app.deliver_signed(&body, header.as_deref()).await;
        refused(&reply, StatusCode::BAD_REQUEST);
        assert_eq!(orders_recorded(&app).await, 0, "{what}");
    }
    // Signed five minutes and a second ago, or as far ahead: a replay,
    // or a clock that is off.
    for at in [NOW - 301, NOW + 301] {
        let reply = app
            .deliver_signed(&body, Some(&signed(SECRET, at, &body)))
            .await;
        refused(&reply, StatusCode::BAD_REQUEST);
    }
    assert_eq!(orders_recorded(&app).await, 0);
    // Within five minutes either way it is believed.
    for (at, name) in [
        (NOW - 300, "checkout-payment"),
        (NOW + 300, "charge-payment"),
    ] {
        let body = event(name, &id);
        let reply = app
            .deliver_signed(&body, Some(&signed(SECRET, at, &body)))
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{name}: {}", reply.body);
    }
    assert_eq!(orders_recorded(&app).await, 1);
    // Only by POST.
    let mut browser = Browser::new([192, 0, 2, 45]);
    let fetched = browser.get(&app, "/stripe/webhook").await;
    assert_eq!(fetched.status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn while_a_secret_is_rolled_deliveries_under_either_are_believed() {
    let app = TestApp::shop_with(&[SECRET, OTHER_SECRET], ClientIpSource::Peer);
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    for (secret, name) in [
        (OTHER_SECRET, "checkout-payment"),
        (SECRET, "charge-payment"),
    ] {
        let body = event(name, &id);
        let reply = app
            .deliver_signed(&body, Some(&signature(secret, NOW, &body)))
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{name}");
    }
    let third = "whsec_test_third_00000000000000000";
    let body = event("charge-refunded-part", &id);
    let reply = app
        .deliver_signed(&body, Some(&signature(third, NOW, &body)))
        .await;
    refused(&reply, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn what_stripe_signed_but_the_portal_cannot_act_on_is_refused_loudly() {
    capture_logs();
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    let good = event("checkout-payment", &id);
    // Each is said once at the level an operator watches, with what kind
    // of thing is wrong.
    let said_loudly = |said: &str| {
        logged()
            .lines()
            .filter(|line| {
                line.contains("ERROR")
                    && line.contains("refused an event Stripe signed")
                    && line.contains(said)
            })
            .count()
    };
    for (body, said) in [
        // Rendered for the release before the one the portal reads.
        (
            good.replace("2026-10-28.endive", "2026-03-25.dahlia"),
            "rendered for another API release",
        ),
        // Of live mode, while the database holds a rehearsal's payments.
        (live(&good), "of the other mode"),
        // An object that is not a checkout's.
        (
            good.replace("\"amount_total\": 800", "\"amount_total\": \"800\""),
            "an object the portal cannot read",
        ),
        // Not an event at all.
        (
            "{\"object\": \"v2.core.event\"}".to_owned(),
            "does not parse",
        ),
        ("not json".to_owned(), "does not parse"),
    ] {
        assert_ne!(body, good, "{said}");
        let before = said_loudly(said);
        let reply = app.deliver(&body).await;
        refused(&reply, StatusCode::BAD_REQUEST);
        assert_eq!(said_loudly(said), before + 1, "{said}: {}", logged());
    }
    assert_eq!(orders_recorded(&app).await, 0);
    // Nothing of a delivery reaches the log but what kind of thing is
    // wrong with it: not the payer, not the order, not the signature.
    let log = logged();
    assert!(log.contains("refused an event Stripe signed"), "{log}");
    for private in [
        "payer@example.test",
        "Synthetic Payer",
        "cs_test_a1SynthPayment",
        "pi_3SynthPayment",
        id.as_str(),
        "v1=",
        SECRET,
    ] {
        assert!(!log.contains(private), "{private}: {log}");
    }
    // Put right, the same event is taken: nothing was remembered of it.
    assert_eq!(app.deliver(&good).await.status, StatusCode::OK);
    assert_eq!(orders_recorded(&app).await, 1);
}

#[tokio::test]
async fn a_delivery_of_real_money_to_a_rehearsal_or_the_reverse_leaves_nothing() {
    capture_logs();
    // A portal of real payments is sent a rehearsal's events.
    let app = TestApp::live_shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    for name in ["checkout-payment", "charge-payment"] {
        let reply = app.deliver(&event(name, &id)).await;
        refused(&reply, StatusCode::BAD_REQUEST);
    }
    assert!(logged().contains("of the other mode"), "{}", logged());
    assert_eq!(orders_recorded(&app).await, 0);
    let page = browser.order_page(&app, &id).await.body;
    assert!(page.contains("待付款"), "{page}");
    // Nothing of them was kept: the same events for real money are taken.
    for name in ["checkout-payment", "charge-payment"] {
        let reply = app.deliver(&live(&event(name, &id))).await;
        assert_eq!(reply.status, StatusCode::OK, "{name}: {}", reply.body);
    }
    assert_eq!(orders_recorded(&app).await, 1);
    // Mooring is told so by the database, entry for entry.
    let bearer = format!("Bearer {}", app.pull_token().await);
    let pulled = app.pull(&format!("{FEED}?after=0"), Some(&bearer)).await;
    assert_eq!(pulled.status, StatusCode::OK);
    assert!(
        pulled.body.contains("\"livemode\":true,"),
        "{}",
        pulled.body
    );
    assert!(
        pulled.body.contains("\"kind\":\"grant\""),
        "{}",
        pulled.body
    );
    // And its pages do not say that it is a rehearsal.
    assert!(!page.contains("测试模式"), "{page}");
}

#[tokio::test]
async fn a_delivery_larger_than_an_event_may_be_is_refused_unread() {
    capture_logs();
    let app = TestApp::shop();
    let large = "x".repeat(512 * 1024 + 1);
    let reply = app.deliver(&large).await;
    refused(&reply, StatusCode::BAD_REQUEST);
    // It was not read to the end, so it was never taken for Stripe's.
    let log = logged();
    assert!(
        log.contains("the delivery is too large or unreadable"),
        "{log}"
    );
    assert!(!log.contains("refused an event Stripe signed"), "{log}");
    // One of exactly the size is read, and is no event.
    let full = "x".repeat(512 * 1024);
    refused(&app.deliver(&full).await, StatusCode::BAD_REQUEST);
    let log = logged();
    assert!(log.contains("refused an event Stripe signed"), "{log}");
}

#[tokio::test]
async fn deliveries_are_limited_per_network_before_they_are_read() {
    let app = TestApp::shop();
    let body = event("unused-type", FIXTURE_ORDER);
    let from = |last: u8| {
        Browser::new([192, 0, 2, last])
            .request("POST", "/stripe/webhook")
            .header("stripe-signature", signature(SECRET, NOW, &body))
            .body(Body::from(body.clone()))
            .unwrap()
    };
    // Unsigned junk uses the allowance up as well as anything.
    for _ in 0..WEBHOOKS_PER_NETWORK.limit {
        let junk = Browser::new([192, 0, 2, 50])
            .request("POST", "/stripe/webhook")
            .body(Body::from("junk"))
            .unwrap();
        assert_eq!(app.send(junk).await.status, StatusCode::BAD_REQUEST);
    }
    let limited = app.send(from(50)).await;
    refused(&limited, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers[RETRY_AFTER], "3600");
    // Another network's deliveries are taken.
    assert_eq!(app.send(from(51)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn behind_a_proxy_a_delivery_is_counted_against_the_address_the_proxy_names() {
    let app = TestApp::shop_with(&[SECRET], ClientIpSource::ForwardedFor);
    let body = event("unused-type", FIXTURE_ORDER);
    let delivery = |forwarded: Option<&str>| {
        let mut browser = Browser::new([127, 0, 0, 1]);
        browser.peer.set_port(41_000);
        let mut request = browser
            .request("POST", "/stripe/webhook")
            .header("stripe-signature", signature(SECRET, NOW, &body));
        if let Some(forwarded) = forwarded {
            request = request.header("x-forwarded-for", forwarded);
        }
        request.body(Body::from(body.clone())).unwrap()
    };
    assert_eq!(
        app.send(delivery(Some("192.0.2.44"))).await.status,
        StatusCode::OK
    );
    // A proxy that names no one is a fault of the set-up, not a delivery.
    let unnamed = app.send(delivery(None)).await;
    refused(&unnamed, StatusCode::INTERNAL_SERVER_ERROR);
}

/// A portal with one granted order, and Mooring's token.
async fn granted() -> (TestApp, String, String) {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment", "charge-payment"], &id)
        .await;
    let token = app.pull_token().await;
    (app, id, format!("Bearer {token}"))
}

#[tokio::test]
async fn mooring_pulls_what_to_grant_as_the_contract_writes_it() {
    let (app, id, bearer) = granted().await;
    let pulled = app.pull(&format!("{FEED}?after=0"), Some(&bearer)).await;
    assert_eq!(pulled.status, StatusCode::OK);
    assert_eq!(pulled.headers[CONTENT_TYPE], "application/json");
    assert_eq!(pulled.headers["cache-control"], "no-store");
    assert!(pulled.headers.get(SET_COOKIE).is_none());
    // The charge was delivered at the test's clock, a second after which
    // nothing passes: the entry is recorded then.
    assert_eq!(
        pulled.body,
        format!(
            "{{\"format\":\"mooring-portal-entitlements\",\"version\":1,\"generated_at\":{NOW},\
             \"livemode\":false,\"after\":0,\"entries\":[{{\"seq\":1,\"kind\":\"grant\",\
             \"order\":\"{id}\",\"user\":\"pilotusera\",\"plan\":\"month-100\",\
             \"plan_kind\":\"month\",\"days\":30,\"traffic_bytes\":100000000000,\"amount\":800,\
             \"currency\":\"gbp\",\"paid_at\":1794355260,\"renewal_of\":null,\
             \"amount_refunded\":null,\"refunded_in_full\":null,\"amount_disputed\":null,\
             \"recorded_at\":{NOW}}}],\"next\":1,\"more\":false}}"
        )
    );
    // After it, nothing new.
    let empty = app.pull(&format!("{FEED}?after=1"), Some(&bearer)).await;
    assert_eq!(
        empty.body,
        format!(
            "{{\"format\":\"mooring-portal-entitlements\",\"version\":1,\"generated_at\":{NOW},\
             \"livemode\":false,\"after\":1,\"entries\":[],\"next\":1,\"more\":false}}"
        )
    );
    // A refund is entered after the grant.
    app.deliver_events(&["charge-refunded-part"], &id).await;
    let refund = app
        .pull(&format!("{FEED}?after=1"), Some(&bearer))
        .await
        .body;
    assert!(
        refund.contains("\"seq\":2,\"kind\":\"refund\"")
            && refund.contains("\"amount_refunded\":300,\"refunded_in_full\":false,"),
        "{refund}"
    );
    // And the rest of it, which the journal says is all.
    app.deliver_events(&["charge-refunded-all"], &id).await;
    let all = app
        .pull(&format!("{FEED}?after=2"), Some(&bearer))
        .await
        .body;
    assert!(
        all.contains("\"seq\":3,\"kind\":\"refund\"")
            && all.contains("\"amount_refunded\":800,\"refunded_in_full\":true,"),
        "{all}"
    );
}

#[tokio::test]
async fn only_the_bearer_of_moorings_token_pulls() {
    let (app, id, bearer) = granted().await;
    let token = bearer.strip_prefix("Bearer ").unwrap();
    let another = "0".repeat(64);
    for authorization in [
        None,
        Some(format!("Bearer {another}")),
        Some(format!("Bearer {}", &token[..63])),
        Some(format!("Bearer {}", token.to_uppercase())),
        Some(format!("bearer {token}")),
        Some(format!("Basic {token}")),
        Some(token.to_owned()),
        Some(format!("Bearer  {token}")),
        Some("Bearer".to_owned()),
        Some(String::new()),
    ] {
        // Whatever else is wrong with the request, it is the token that
        // is asked for first.
        for path in [
            format!("{FEED}?after=0"),
            FEED.to_owned(),
            format!("{FEED}?after=x"),
        ] {
            let reply = app.pull(&path, authorization.as_deref()).await;
            refused(&reply, StatusCode::UNAUTHORIZED);
            assert_eq!(reply.headers[WWW_AUTHENTICATE], "Bearer");
            assert!(!reply.body.contains(&id), "{authorization:?}");
        }
    }
    // The token is not taken from the address.
    let in_address = app
        .pull(&format!("{FEED}?after=0&token={token}"), None)
        .await;
    refused(&in_address, StatusCode::UNAUTHORIZED);
    // Nothing a refused pull asked for was noted.
    let state = app.store(|store| store.pull_state()).await.unwrap();
    assert_eq!((state.pulled_through, state.pulled_at), (0, None));
    // Revoked, the token pulls no more; with no token set, nothing does.
    assert_eq!(
        app.pull(&format!("{FEED}?after=0"), Some(&bearer))
            .await
            .status,
        StatusCode::OK
    );
    app.store(|store| store.revoke_pull_token()).await.unwrap();
    refused(
        &app.pull(&format!("{FEED}?after=0"), Some(&bearer)).await,
        StatusCode::UNAUTHORIZED,
    );
}

#[tokio::test]
async fn a_pull_must_say_after_which_entry_and_not_one_the_journal_never_wrote() {
    capture_logs();
    let (app, _, bearer) = granted().await;
    for path in [
        FEED.to_owned(),
        format!("{FEED}?after="),
        format!("{FEED}?after=-1"),
        format!("{FEED}?after=1.5"),
        format!("{FEED}?after=one"),
        format!("{FEED}?after=18446744073709551616"),
        format!("{FEED}?before=1"),
        format!("{FEED}?after=0&after=1"),
    ] {
        let reply = app.pull(&path, Some(&bearer)).await;
        refused(&reply, StatusCode::BAD_REQUEST);
    }
    // A key the portal does not know beside it does no harm.
    assert_eq!(
        app.pull(&format!("{FEED}?after=1&limit=5"), Some(&bearer))
            .await
            .status,
        StatusCode::OK
    );
    // A cursor beyond the journal: not this journal's.
    let ahead = app.pull(&format!("{FEED}?after=2"), Some(&bearer)).await;
    refused(&ahead, StatusCode::CONFLICT);
    assert!(logged().contains("refused a pull"), "{}", logged());
    let state = app.store(|store| store.pull_state()).await.unwrap();
    assert_eq!(state.pulled_through, 1);
    // The token is in no log.
    let token = bearer.strip_prefix("Bearer ").unwrap();
    assert!(!logged().contains(token));
    // Only by GET.
    let posted = Browser::new([192, 0, 2, 99])
        .request("POST", &format!("{FEED}?after=0"))
        .header("authorization", bearer.as_str())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.send(posted).await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn pulls_are_limited_per_network_before_the_token_is_read() {
    let (app, _, bearer) = granted().await;
    let path = format!("{FEED}?after=0");
    for _ in 0..PULLS_PER_NETWORK.limit - 1 {
        // Guesses use the allowance up as well as pulls do.
        assert_eq!(app.pull(&path, None).await.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(app.pull(&path, Some(&bearer)).await.status, StatusCode::OK);
    let limited = app.pull(&path, Some(&bearer)).await;
    refused(&limited, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers[RETRY_AFTER], "3600");
}

#[tokio::test]
async fn a_long_journal_is_pulled_a_page_at_a_time() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment", "charge-payment"], &id)
        .await;
    // Two hundred and five refunds, each of a penny more.
    for penny in 1..=205_u32 {
        let refund = event("charge-refunded-part", &id)
            .replace(
                "evt_1SynthChargeRefundedPart",
                &format!("evt_refund{penny}"),
            )
            .replace(
                "\"amount_refunded\": 300",
                &format!("\"amount_refunded\": {penny}"),
            );
        assert_eq!(app.deliver(&refund).await.status, StatusCode::OK);
    }
    let bearer = format!("Bearer {}", app.pull_token().await);
    let first = app
        .pull(&format!("{FEED}?after=0"), Some(&bearer))
        .await
        .body;
    assert_eq!(first.matches("\"seq\":").count(), 200);
    assert!(
        first.ends_with("\"next\":200,\"more\":true}"),
        "{}",
        &first[first.len() - 60..]
    );
    let rest = app
        .pull(&format!("{FEED}?after=200"), Some(&bearer))
        .await
        .body;
    assert_eq!(rest.matches("\"seq\":").count(), 6);
    assert!(rest.ends_with("\"next\":206,\"more\":false}"));
}

#[tokio::test]
async fn a_database_that_fails_is_a_failure_and_never_a_yes() {
    let (app, file) = TestApp::shop_on_disk();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment", "charge-payment"], &id)
        .await;
    let bearer = format!("Bearer {}", app.pull_token().await);
    // Behind the portal's back, as a fault of the disk would.
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "DROP TABLE entitlements; DROP TABLE stripe_events; DROP TABLE pull_access; \
         DROP TABLE orders;",
    )
    .unwrap();
    // Stripe is told to deliver again, not that the event was taken.
    let delivery = app.deliver(&event("charge-refunded-part", &id)).await;
    refused(&delivery, StatusCode::INTERNAL_SERVER_ERROR);
    // Mooring is not told that there is nothing new, nor that its token
    // is wrong.
    for path in [format!("{FEED}?after=0"), FEED.to_owned()] {
        let pulled = app.pull(&path, Some(&bearer)).await;
        refused(&pulled, StatusCode::INTERNAL_SERVER_ERROR);
    }
    // And a customer is shown an error, not an empty shop or a new order.
    let ordered = browser
        .post(&app, "/subscription/orders", &[("plan", "month-100")])
        .await;
    assert_eq!(ordered.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(ordered.body.contains("服务器出错了"));
    for path in [
        format!("/subscription/orders/{id}"),
        "/subscription".to_owned(),
    ] {
        let page = browser.get(&app, &path).await;
        assert_eq!(page.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
        assert!(!page.body.contains("购买与续费"), "{path}");
    }
}
