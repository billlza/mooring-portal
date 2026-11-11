//! Starting a portal over a database: which money it holds, and what a
//! portal that sells nothing must not be started over.

use std::sync::Arc;

use axum::http::StatusCode;
use portal_core::auth::HashCost;
use portal_core::payment::{OrderId, ReviewReason};
use portal_core::time::{ManualClock, UnixTime};
use portal_store::{ModeError, OpenMode, Progress, Store, StoreError};

use super::paying::{catalogue, event, live, live_catalogue, payments_on};
use super::support::{DatabaseFile, NOW, ORIGIN_URL, TestApp};
use crate::config::{ClientIpSource, Payments, PublicOrigin, WebConfig};
use crate::error::AppError;
use crate::state::{AppState, StartupError};

/// Starts a portal over the database `file`, selling what `payments`
/// holds if any.
fn start(file: &DatabaseFile, payments: Option<Payments>) -> Result<AppState, StartupError> {
    AppState::new(
        Store::open(file.path(), OpenMode::Existing).unwrap(),
        WebConfig {
            origin: PublicOrigin::parse(ORIGIN_URL).unwrap(),
            client_ip: ClientIpSource::Peer,
            max_accounts: 100,
            payments,
        },
        HashCost::MINIMUM_FOR_TESTS,
        Arc::new(ManualClock::new(UnixTime::from_secs(NOW).unwrap())),
    )
}

#[tokio::test]
async fn only_a_portal_that_sells_has_what_it_sells() {
    let shop = TestApp::shop();
    assert_eq!(shop.state.selling().unwrap().catalogue().plans().len(), 3);
    let plain = TestApp::new();
    assert!(matches!(plain.state.selling(), Err(AppError::NotFound)));
}

#[tokio::test]
async fn a_portal_does_not_start_over_payments_of_the_other_mode() {
    // A rehearsal: one order, not even paid.
    let (app, file) = TestApp::shop_on_disk();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    drop(app);
    // The catalogue is changed for the real one, over the same database.
    let refused = start(&file, Some(payments_on(live_catalogue())));
    assert!(
        matches!(
            refused,
            Err(StartupError::PaymentMode(ModeError::Held {
                livemode: false
            }))
        ),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the database holds orders and payments of Stripe's test mode, and the catalogue is for \
         real payments: remove them with `discard-test-payments` first"
    );
    // With the rehearsal's catalogue it starts as before, and its events
    // are still the database's.
    let app = TestApp::reopened(&file, Some(payments_on(catalogue())));
    app.deliver_events(&["checkout-payment"], &id).await;
    drop(app);
    // The rehearsal discarded, it starts for real money; what Stripe
    // sends of the rehearsal after that is refused.
    Store::open(file.path(), OpenMode::Existing)
        .unwrap()
        .discard_test_payments()
        .unwrap();
    let app = TestApp::reopened(&file, Some(payments_on(live_catalogue())));
    let late = app.deliver(&event("charge-payment", &id)).await;
    assert_eq!(late.status, StatusCode::BAD_REQUEST);
    let real = app.deliver(&live(&event("charge-payment", &id))).await;
    assert_eq!(real.status, StatusCode::OK);
}

#[tokio::test]
async fn a_portal_that_sells_nothing_does_not_start_over_money_in_the_air() {
    let (app, file) = TestApp::shop_on_disk();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    // An order nobody paid is no money: without the catalogue the portal
    // starts, and shows no shop.
    let id = browser.order(&app, "month-100").await;
    drop(app);
    let plain = TestApp::reopened(&file, None);
    let page = browser.get(&plain, "/subscription").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(!page.body.contains("购买与续费"));
    drop(plain);
    // Paid: Stripe has more to say of it, and Mooring has not fetched it.
    let app = TestApp::reopened(&file, Some(payments_on(catalogue())));
    app.deliver_events(&["checkout-payment", "charge-payment"], &id)
        .await;
    let bearer = format!("Bearer {}", app.pull_token().await);
    drop(app);
    let refused = start(&file, None);
    assert!(
        matches!(refused, Err(StartupError::PaymentsOutstanding)),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the database has subscriptions that Stripe may charge again, or orders that are not \
         settled (`orders` lists them), and the server was started without payments: start it \
         with `--payments` and `--stripe-webhook-secret`; a rehearsal's orders are removed with \
         `discard-test-payments`"
    );
    // Once Mooring has it, nothing is in the air.
    let app = TestApp::reopened(&file, Some(payments_on(catalogue())));
    let pulled = app
        .pull("/api/mooring/entitlements?after=1", Some(&bearer))
        .await;
    assert_eq!(pulled.status, StatusCode::OK);
    drop(app);
    assert!(start(&file, None).is_ok());
}

#[tokio::test]
async fn a_database_that_fails_at_start_is_a_failure() {
    let (app, file) = TestApp::shop_on_disk();
    drop(app);
    rusqlite::Connection::open(file.path())
        .unwrap()
        .execute_batch("DROP TABLE orders;")
        .unwrap();
    let selling = start(&file, Some(payments_on(live_catalogue())));
    assert!(
        matches!(
            selling,
            Err(StartupError::PaymentMode(ModeError::Store(
                StoreError::Sqlite(_)
            )))
        ),
        "{selling:?}"
    );
    let plain = start(&file, None);
    assert!(
        matches!(plain, Err(StartupError::Store(StoreError::Sqlite(_)))),
        "{plain:?}"
    );
}

#[tokio::test]
async fn an_order_paid_after_its_plans_terms_changed_waits_for_the_operator() {
    let (app, file) = TestApp::shop_on_disk();
    let (mut browser, code) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    drop(app);
    // The month is dearer now.
    let written = std::fs::read_to_string(
        [
            env!("CARGO_MANIFEST_DIR"),
            "../../contract/fixtures/payments/demo.json",
        ]
        .iter()
        .collect::<std::path::PathBuf>(),
    )
    .unwrap();
    let dearer = written.replacen("\"amount\": 800", "\"amount\": 900", 1);
    assert_ne!(dearer, written);
    let dearer = portal_core::payment::Catalogue::parse(&dearer).unwrap();
    let app = TestApp::reopened(&file, Some(payments_on(dearer)));
    // The order's page no longer leads to Stripe.
    let page = browser.order_page(&app, &id).await.body;
    assert!(!page.contains("buy.stripe.com"), "{page}");
    assert!(page.contains("这张订单不能再付款"), "{page}");
    // Paid all the same, from an address kept: Stripe's word is taken,
    // and the order is the operator's to look at.
    app.deliver_events(&["checkout-payment", "charge-payment"], &id)
        .await;
    let order = OrderId::parse(&id).unwrap();
    let progress = app
        .store(move |store| {
            let account = store
                .accounts()
                .unwrap()
                .into_iter()
                .find(|account| account.code == code)
                .unwrap();
            let listed = store.recent_orders(10).unwrap();
            assert_eq!(listed[0].account, account.code);
            listed
                .into_iter()
                .find(|listed| listed.order.id == order)
                .unwrap()
                .order
                .progress
        })
        .await;
    assert_eq!(progress, Progress::Review(ReviewReason::Terms));
    let page = browser.order_page(&app, &id).await.body;
    assert!(page.contains("需要运营者核对"), "{page}");
    // Ordered again, it is on the new terms.
    let again = browser.order(&app, "month-100").await;
    assert_ne!(again, id);
    let page = browser.order_page(&app, &again).await.body;
    assert!(page.contains("£9.00"), "{page}");
}
