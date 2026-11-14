//! Renewals whose first charge fails, told of by Stripe and pulled by
//! Mooring.

use axum::http::StatusCode;
use portal_core::payment::{Catalogue, ReviewReason};
use portal_store::Progress;

use super::machines::{FEED, orders_recorded};
use super::paying::{event, payments_on};
use super::support::TestApp;

#[tokio::test]
async fn a_commitment_keeps_the_renewal_price_promised_at_checkout() {
    let payments = |renewal_amount| {
        payments_on(Catalogue::parse(&format!(
            r#"{{"format":"mooring-portal-payments","version":1,"livemode":false,"currency":"gbp",
            "plans":[{{"id":"term-100","kind":"month_auto","days":30,"traffic_gb":100,"amount":800,
            "renewal_amount":{renewal_amount},
            "payment_link":"https://buy.stripe.com/test_SynthAuto","payment_link_id":"plink_1SynthAuto"}}],
            "manage_subscription_url":"https://billing.stripe.com/p/login/test_SynthManage"}}"#
        )).unwrap())
    };
    let (app, file) = TestApp::on_disk_with(Some(payments(1350)));
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let first = browser.order(&app, "term-100").await;
    let page = browser.order_page(&app, &first).await;
    assert_eq!(page.status, StatusCode::OK);
    for figure in ["£8.00", "£13.50"] {
        assert!(page.body.contains(figure), "{}", page.body);
    }
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
    drop(app);

    // A restart with a new price for future buyers must not reprice an
    // existing subscription. Replay the renewal to also check deduplication.
    let app = TestApp::reopened(&file, Some(payments(1400)));
    let paid = event("invoice-paid-cycle", &first).replace(": 800", ": 1350");
    for _ in 0..2 {
        assert_eq!(app.deliver(&paid).await.status, StatusCode::OK);
        app.deliver_events(&["invoice-payment-cycle", "charge-cycle-invoice"], &first)
            .await;
    }

    // The original £8 is valid only at checkout. Charging it again must
    // go to review even when the card used for that invoice checks out.
    for name in [
        "invoice-paid-cycle",
        "invoice-payment-cycle",
        "charge-cycle-invoice",
    ] {
        let next = event(name, &first)
            .replace("evt_1Synth", "evt_1Next")
            .replace("in_1SynthCycle", "in_1NextCycle")
            .replace("pi_1SynthCycle", "pi_1NextCycle");
        assert_eq!(app.deliver(&next).await.status, StatusCode::OK);
    }
    let orders = app.store(|store| store.recent_orders(10)).await.unwrap();
    assert_eq!(orders.len(), 3);
    assert_eq!(
        orders
            .iter()
            .find(|row| row
                .invoice
                .as_ref()
                .is_some_and(|id| id.as_str() == "in_1NextCycle"))
            .unwrap()
            .order
            .progress,
        Progress::Review(ReviewReason::Amount),
    );
    let feed = app.pull(&format!("{FEED}?after=0"), Some(&bearer)).await;
    assert_eq!(feed.status, StatusCode::OK);
    let renewal = orders
        .iter()
        .find(|row| {
            row.invoice
                .as_ref()
                .is_some_and(|id| id.as_str() == "in_1SynthCycle")
        })
        .unwrap()
        .order
        .id
        .as_str();
    for (seq, order, amount) in [(1, first.as_str(), 800), (2, renewal, 1350)] {
        let entry = format!(
            concat!(
                "\"seq\":{},\"kind\":\"grant\",\"order\":\"{}\",\"user\":\"pilotusera\",",
                "\"plan\":\"term-100\",\"plan_kind\":\"month_auto\",\"days\":30,",
                "\"traffic_bytes\":100000000000,\"amount\":{},\"currency\":\"gbp\"",
            ),
            seq, order, amount
        );
        assert!(feed.body.contains(&entry), "{}", feed.body);
    }
    assert_eq!(
        feed.body.matches("\"kind\":\"grant\"").count(),
        2,
        "{}",
        feed.body
    );
    assert!(
        feed.body.contains(&format!("\"renewal_of\":\"{first}\"")),
        "{}",
        feed.body
    );
}

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
