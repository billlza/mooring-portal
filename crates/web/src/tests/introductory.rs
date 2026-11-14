//! New-user offers, an off-session schedule and the non-renewal request,
//! through the signed webhook, the owner pages and the entitlements feed.

use axum::http::StatusCode;
use portal_core::payment::Catalogue;
use portal_core::payment::stripe::StripeId;
use portal_core::snapshot::MooringUserId;
use portal_store::{IntroError, IntroLink};

use super::machines::FEED;
use super::paying::{event, payments_on};
use super::support::{NOW, TestApp};
use crate::config::ClientIpSource;

const TERM: &str = "term-100";
const STOP: &str = "/subscription/intro/non-renewal";
const LINK: &str = "plink_1PrivateOffer";
const PAY_URL: &str = "https://buy.stripe.com/test_PrivateOffer";
const DAYS: u64 = 86_400;

fn app() -> TestApp {
    let catalogue = Catalogue::parse(r#"{
        "format":"mooring-portal-payments","version":1,"livemode":false,"currency":"gbp",
        "plans":[
            {"id":"term-100","kind":"month_auto","days":30,"traffic_gb":100,"amount":800,
             "renewal_amount":1350,"introductory":true,"payment_link":"https://buy.stripe.com/test_Placeholder",
             "payment_link_id":"plink_1Placeholder"},
            {"id":"month-100","kind":"month","days":30,"traffic_gb":100,"amount":800,
             "payment_link":"https://buy.stripe.com/test_SynthMonth","payment_link_id":"plink_1SynthMonth"}],
        "manage_subscription_url":"https://billing.stripe.com/p/login/test_Manage"}"#).unwrap();
    TestApp::selling(payments_on(catalogue), ClientIpSource::Peer)
}

fn private_link() -> IntroLink {
    IntroLink::parse(StripeId::parse(LINK).unwrap(), PAY_URL.to_owned()).unwrap()
}

async fn deliver(app: &TestApp, id: &str, kind: &str, at: u64, object: &str) {
    let raw = format!(
        r#"{{"id":"evt_{id}","type":"{kind}","created":{at},"livemode":false,
        "api_version":"2026-10-28.endive","data":{{"object":{object}}}}}"#
    );
    assert_eq!(app.deliver(&raw).await.status, StatusCode::OK, "{id}");
}

fn schedule(order: &str, customer: &str, end: u64, mode: &str, sub: &str) -> String {
    let start = NOW + 30 * DAYS;
    format!(
        r#"{{"id":"sub_sched_Sandbox","customer":"{customer}","status":"active",
        "end_behavior":"{mode}","subscription":{sub},"metadata":{{"moorlight_intro_order":"{order}"}},
        "phases":[{{"start_date":{start},"end_date":{end}}}]}}"#
    )
}

#[tokio::test]
async fn a_private_offer_can_only_grant_its_bound_user_and_the_stop_requires_stripes_word() {
    let app = app();
    let (mut new, new_code) = app.customer("new@qq.com", "pilotusera").await;
    let (mut old, old_code) = app.customer("old@qq.com", "pilotuserb").await;
    assert_eq!(
        old.post(&app, "/subscription/orders", &[("plan", TERM)])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let old_order = old.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment", "charge-payment"], &old_order)
        .await;
    let refused = app
        .store(move |s| {
            s.issue_introductory_offer(
                &old_code,
                &MooringUserId::parse("pilotuserb").unwrap(),
                &private_link(),
            )
        })
        .await;
    assert!(matches!(refused, Err(IntroError::Used)));
    app.store(move |s| {
        s.issue_introductory_offer(
            &new_code,
            &MooringUserId::parse("pilotusera").unwrap(),
            &private_link(),
        )
    })
    .await
    .unwrap();
    assert!(
        new.get(&app, "/subscription")
            .await
            .body
            .contains("£156.50")
    );
    assert!(
        !old.get(&app, "/subscription")
            .await
            .body
            .contains("£156.50")
    );
    assert!(!new.get(&app, "/plans").await.body.contains("£156.50"));
    let order = new.order(&app, TERM).await;
    assert!(
        new.order_page(&app, &order)
            .await
            .body
            .contains(&format!("{PAY_URL}?client_reference_id={order}"))
    );
    assert_eq!(
        old.order_page(&app, &order).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        old.post(&app, STOP, &[]).await.status,
        StatusCode::NOT_FOUND
    );

    let another = old.order(&app, "month-100").await;
    let wrong = event("checkout-payment", &another)
        .replace("plink_1SynthMonth", LINK)
        .replace("evt_1SynthCheckoutPayment", "evt_Forwarded")
        .replace("cs_test_a1SynthPayment", "cs_test_Forwarded")
        .replace("pi_3SynthPayment", "pi_Forwarded");
    assert_eq!(app.deliver(&wrong).await.status, StatusCode::OK);
    assert_eq!(app.store(|s| s.stray_payments()).await.unwrap().len(), 1);

    let checkout = format!(
        r#"{{"id":"cs_test_Intro","mode":"payment","payment_status":"paid",
        "client_reference_id":"{order}","payment_link":"{LINK}","payment_intent":"pi_Intro",
        "amount_total":800,"currency":"gbp","customer":"cus_Intro"}}"#
    );
    deliver(
        &app,
        "IntroCheckout",
        "checkout.session.completed",
        NOW + 4,
        &checkout,
    )
    .await;
    deliver(&app, "IntroCharge", "charge.succeeded", NOW, r#"{
        "payment_intent":"pi_Intro","payment_method_details":{"type":"card","card":{"fingerprint":"clear123456"}}}"#).await;
    let end = NOW + 360 * DAYS;
    assert_eq!(app.store(|s| s.pull_state()).await.unwrap().last_seq, 1);
    deliver(
        &app,
        "IntroPaid",
        "payment_intent.succeeded",
        NOW,
        r#"{
        "id":"pi_Intro","customer":"cus_Intro","payment_method":"pm_Intro",
        "setup_future_usage":"off_session","metadata":{"moorlight_offer":"intro_v2"}}"#,
    )
    .await;
    assert_eq!(
        new.post(&app, STOP, &[]).await.status,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        new.post(&app, STOP, &[]).await.status,
        StatusCode::SEE_OTHER
    );
    assert!(
        new.get(&app, "/subscription")
            .await
            .body
            .contains("等待 Stripe 确认")
    );
    assert_eq!(
        app.store(|s| s.introductory_requests())
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        app.store(|s| s.introductory_requests()).await.unwrap()[0]
            .schedule
            .is_none()
    );
    assert!(app.store(|s| s.introductory_starts()).await.unwrap()[0].stop);
    let starts = schedule(&order, "cus_Intro", end, "release", "null");
    deliver(
        &app,
        "IntroSchedStart",
        "subscription_schedule.created",
        NOW + 6,
        &starts,
    )
    .await;
    assert_eq!(app.store(|s| s.pull_state()).await.unwrap().last_seq, 2);
    let wrong_date = schedule(&order, "cus_Intro", end - 30 * DAYS, "cancel", "null");
    deliver(
        &app,
        "IntroSchedWrong",
        "subscription_schedule.updated",
        NOW + 7,
        &wrong_date,
    )
    .await;
    assert!(
        new.get(&app, "/subscription")
            .await
            .body
            .contains("等待 Stripe 确认")
    );
    let stop = schedule(&order, "cus_Intro", end, "cancel", "null");
    deliver(
        &app,
        "IntroSchedStop",
        "subscription_schedule.updated",
        NOW + 8,
        &stop,
    )
    .await;
    assert!(
        new.get(&app, "/subscription")
            .await
            .body
            .contains("不会扣第 13 笔")
    );
    assert!(
        app.store(|s| s.introductory_requests())
            .await
            .unwrap()
            .is_empty()
    );

    // The schedule starts a subscription at day 30. Its first invoice
    // is period two of the contract, even if it arrives before the link.
    let cycle = format!(
        r#"{{"id":"in_IntroCycle","amount_paid":1350,"currency":"gbp",
        "billing_reason":"subscription_create","parent":{{"subscription_details":{{
        "subscription":"sub_Intro","metadata":{{"moorlight_intro_order":"{order}"}}}}}}}}"#
    );
    deliver(&app, "IntroCycle", "invoice.paid", NOW + 30 * DAYS, &cycle).await;
    deliver(&app, "IntroCyclePaidBy", "invoice_payment.paid", NOW + 30 * DAYS,
        r#"{"invoice":"in_IntroCycle","payment":{"type":"payment_intent","payment_intent":"pi_IntroCycle"}}"#).await;
    deliver(&app, "IntroCycleCharge", "charge.succeeded", NOW + 30 * DAYS, r#"{
        "payment_intent":"pi_IntroCycle","payment_method_details":{"type":"card","card":{"fingerprint":"clear123456"}}}"#).await;
    let active = schedule(&order, "cus_Intro", end, "cancel", "\"sub_Intro\"");
    deliver(
        &app,
        "IntroActive",
        "subscription_schedule.updated",
        NOW + 30 * DAYS,
        &active,
    )
    .await;
    let bearer = format!("Bearer {}", app.pull_token().await);
    let feed = app.pull(&format!("{FEED}?after=1"), Some(&bearer)).await;
    assert_eq!(feed.status, StatusCode::OK);
    assert_eq!(
        feed.body.matches("\"kind\":\"grant\"").count(),
        2,
        "{}",
        feed.body
    );
    assert!(feed.body.contains("\"amount\":800") && feed.body.contains("\"amount\":1350"));
    assert!(feed.body.contains(&format!("\"renewal_of\":\"{order}\"")));

    app.store(move |s| s.unbind(&new_code)).await.unwrap();
    let (_, replacement) = app.customer("replacement@qq.com", "pilotusera").await;
    let second = app
        .store(move |s| {
            s.issue_introductory_offer(
                &replacement,
                &MooringUserId::parse("pilotusera").unwrap(),
                &private_link(),
            )
        })
        .await;
    assert!(matches!(second, Err(IntroError::Used)));
}
