//! Renewals whose first charge fails, told of by Stripe and pulled by
//! Mooring.

use axum::http::StatusCode;

use super::machines::{FEED, orders_recorded};
use super::paying::event;
use super::support::TestApp;

#[tokio::test]
async fn a_failed_renewal_only_grants_once_when_its_invoice_is_paid_on_retry() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let first = browser.order(&app, "month-100-auto").await;
    app.deliver_events(
        &[
            "checkout-subscription",
            "invoice-paid-first",
            "invoice-payment-first",
            "charge-first-invoice",
        ],
        &first,
    )
    .await;
    let bearer = format!("Bearer {}", app.pull_token().await);

    let failed = event("invoice-paid-cycle", &first)
        .replace("evt_1SynthInvoicePaidCycle", "evt_1SynthInvoiceFailedCycle")
        .replace("\"amount_paid\": 800", "\"amount_paid\": 0")
        .replace("\"amount_remaining\": 0", "\"amount_remaining\": 800")
        .replace("\"status\": \"paid\"", "\"status\": \"open\"")
        .replace("\"invoice.paid\"", "\"invoice.payment_failed\"");
    let past_due = event("subscription-resumed", &first)
        .replace("\"status\": \"active\"", "\"status\": \"past_due\"");
    assert_eq!(app.deliver(&failed).await.status, StatusCode::OK);
    assert_eq!(app.deliver(&past_due).await.status, StatusCode::OK);
    assert_eq!(orders_recorded(&app).await, 1);
    assert_eq!(
        app.store(|store| store.open_renewals())
            .await
            .unwrap()
            .len(),
        1
    );
    let next = format!("{FEED}?after=1");
    let empty = app.pull(&next, Some(&bearer)).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert!(
        empty.body.contains("\"entries\":[],\"next\":1"),
        "{}",
        empty.body
    );

    let paid = [
        "invoice-paid-cycle",
        "invoice-payment-cycle",
        "charge-cycle-invoice",
    ];
    app.deliver_events(&paid, &first).await;
    app.deliver_events(&paid, &first).await;
    let renewed = app.pull(&next, Some(&bearer)).await;
    assert_eq!(renewed.status, StatusCode::OK);
    assert_eq!(orders_recorded(&app).await, 2);
    for expected in [
        "\"seq\":2,\"kind\":\"grant\"".to_owned(),
        format!("\"renewal_of\":\"{first}\""),
        "\"next\":2".to_owned(),
    ] {
        assert!(renewed.body.contains(&expected), "{}", renewed.body);
    }

    // Another renewal never recovers. Stripe reports that the
    // subscription has ended, without saying this invoice was paid.
    let next_failure = failed
        .replace("SynthInvoiceFailedCycle", "SynthInvoiceFailedLater")
        .replace("in_1SynthCycle", "in_1SynthLater");
    assert_eq!(app.deliver(&next_failure).await.status, StatusCode::OK);
    app.deliver_events(&["subscription-deleted"], &first).await;
    assert_eq!(orders_recorded(&app).await, 2);
    assert!(
        app.store(|store| store.open_renewals())
            .await
            .unwrap()
            .is_empty()
    );
    let ended = app.pull(&format!("{FEED}?after=2"), Some(&bearer)).await;
    assert_eq!(ended.status, StatusCode::OK);
    assert!(
        ended.body.contains("\"entries\":[],\"next\":2"),
        "{}",
        ended.body
    );
}
