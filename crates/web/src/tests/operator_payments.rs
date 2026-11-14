//! The operator's page of payments: what waits and why, approving an
//! order for what the page showed it waits for, and what is refused.

use axum::http::StatusCode;
use portal_core::account::AccountCode;

use super::operator::operator;
use super::paying::event;
use super::support::{Browser, Reply, TestApp};

/// The fixture event `name` for `order`, its ids made the `n`th
/// customer's own: two customers never pay with one checkout.
fn event_of(name: &str, order: &str, n: u8) -> String {
    event(name, order)
        .replace("SynthPayment", &format!("SynthPayment{n}"))
        .replace("evt_1Synth", &format!("evt_1Synth{n}"))
}

/// Delivers each of the fixture events `names` for `order`, as the `n`th
/// customer's own.
async fn deliver(app: &TestApp, names: &[&str], order: &str, n: u8) {
    for name in names {
        let reply = app.deliver(&event_of(name, order, n)).await;
        assert_eq!(reply.status, StatusCode::OK, "{name}: {}", reply.body);
    }
}

/// The checkout of the month for `order`, as the `n`th customer's own,
/// paid at the wrong price.
async fn deliver_short(app: &TestApp, order: &str, n: u8) {
    let short = event_of("checkout-payment", order, n)
        .replace("\"amount_total\": 800", "\"amount_total\": 1");
    assert_ne!(short, event_of("checkout-payment", order, n));
    assert_eq!(app.deliver(&short).await.status, StatusCode::OK);
}

/// A bound customer who ordered the month: its browser, account and
/// order.
async fn ordered(app: &TestApp, email: &str, user: &str) -> (Browser, AccountCode, String) {
    let (mut browser, code) = app.customer(email, user).await;
    let id = browser.order(app, "month-100").await;
    (browser, code, id)
}

/// The approving form for `order`, saying the page showed it waits for
/// `shown`.
async fn approve(op: &mut Browser, app: &TestApp, order: &str, shown: &str) -> Reply {
    op.post(
        app,
        "/operator/payments/approve",
        &[("order", order), ("shown", shown)],
    )
    .await
}

/// The approving form the page writes for `order`, when it shows the
/// order waits for `shown`.
fn form_of(order: &str, shown: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"order\" value=\"{order}\">\n                    <input type=\"hidden\" name=\"shown\" value=\"{shown}\">"
    )
}

#[tokio::test]
async fn the_page_says_why_each_order_waits_and_what_to_do() {
    let app = TestApp::shop();
    let (mut op, _) = operator(&app).await;
    let (_, alice, wallet) = ordered(&app, "alice@qq.com", "pilotusera").await;
    deliver(&app, &["checkout-payment", "charge-wallet"], &wallet, 1).await;
    let (_, _, checking) = ordered(&app, "bob@qq.com", "pilotuserb").await;
    deliver(&app, &["checkout-payment"], &checking, 2).await;
    let (_, _, short) = ordered(&app, "carol@qq.com", "pilotuserc").await;
    deliver_short(&app, &short, 3).await;
    let (_, _, unpaid) = ordered(&app, "dave@qq.com", "pilotuserd").await;
    let page = op.get(&app, "/operator/payments").await;
    assert_eq!(page.status, StatusCode::OK);
    let body = &page.body;
    for (order, shown, why, todo) in [
        (
            &wallet,
            "card_unchecked",
            "用钱包或其他方式付的款，银行卡没法和封禁名单核对。",
            "到 Stripe 后台全额退款，或者放行，不核对银行卡。",
        ),
        (
            &checking,
            "checking",
            "已付款，在等 Stripe 告知扣款的结果（Stripe 的通知事件里有 charge.succeeded 吗？）。",
            "Stripe 后台可以重发通知；一直等不到的话，放行会跳过银行卡的核对。",
        ),
        (
            &short,
            "amount",
            "付的金额或币种和订单的价格不符。",
            "Stripe 还没告知银行卡的核对结果：放行后它会等这个结果。",
        ),
    ] {
        assert!(body.contains(&form_of(order, shown)), "{order}: {body}");
        assert!(body.contains(why), "{why}");
        assert!(body.contains(todo), "{todo}");
    }
    assert!(body.contains(&alice.to_string()));
    assert!(body.contains("结账 <span class=\"mono\">cs_test_a1SynthPayment1</span>"));
    // An order not paid waits for no one, and is only among the latest.
    assert!(!body.contains(&form_of(&unpaid, "checking")));
    assert_eq!(body.matches(&unpaid).count(), 1, "{body}");
    // The tab, the overview and the bar count what waits.
    assert!(
        body.contains("付款 <span class=\"count\">3</span>"),
        "{body}"
    );
    let overview = op.get(&app, "/operator").await.body;
    assert!(overview.contains("等你处理的付款"), "{overview}");
    assert!(overview.contains(">3</p>"), "{overview}");
    assert!(
        overview.contains("运营 <span class=\"count\">3</span>"),
        "{overview}"
    );
}

#[tokio::test]
async fn an_order_is_approved_for_what_the_page_showed_and_nothing_more() {
    let app = TestApp::shop();
    let (mut op, _) = operator(&app).await;
    let (mut carol, _, short) = ordered(&app, "carol@qq.com", "pilotuserc").await;
    deliver_short(&app, &short, 3).await;
    deliver(&app, &["charge-wallet"], &short, 3).await;
    let first = approve(&mut op, &app, &short, "amount").await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert!(
        first.body.contains(&format!(
            "已放行订单 {short}，但它又停下来等你：用钱包或其他方式付的款，银行卡没法和封禁名单核对。"
        )),
        "{}",
        first.body
    );
    assert!(first.body.contains(&form_of(&short, "card_unchecked")));
    // The same form again, as a second click sends it: the order waits
    // for its card now, which that page never showed.
    let again = approve(&mut op, &app, &short, "amount").await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert!(again.body.contains(&format!(
        "没有改动：订单 {short} 在这个页面打开之后有了变化。看一下它现在等的是什么，再决定。"
    )));
    assert!(again.body.contains(&form_of(&short, "card_unchecked")));
    let passed = approve(&mut op, &app, &short, "card_unchecked").await;
    assert_eq!(passed.status, StatusCode::OK);
    assert!(passed.body.contains(&format!(
        "已放行订单 {short}：已记入清单，等 Mooring 开通。"
    )));
    assert!(passed.body.contains("没有等待中的订单。"));
    // The counts on the page are what is left.
    assert!(
        !passed.body.contains("<span class=\"count\">"),
        "{}",
        passed.body
    );
    assert!(
        carol
            .order_page(&app, &short)
            .await
            .body
            .contains("已付款，等待开通")
    );
}

#[tokio::test]
async fn approving_is_refused_for_what_cannot_be_granted() {
    let app = TestApp::shop();
    let (mut op, _) = operator(&app).await;
    // What no page writes is no form of the page's.
    for (order, shown) in [
        ("not an order", "checking"),
        ("0123456789abcdefghjk", "approved"),
        ("0123456789abcdefghjk", ""),
    ] {
        assert_eq!(
            approve(&mut op, &app, order, shown).await.status,
            StatusCode::BAD_REQUEST,
            "{order} {shown}"
        );
    }
    let refused = |reply: &Reply, says: &str| {
        assert_eq!(reply.status, StatusCode::CONFLICT, "{says}");
        assert!(reply.body.contains(says), "{says}: {}", reply.body);
    };
    refused(
        &approve(&mut op, &app, "zzzzzzzzzzzzzzzzzzzz", "checking").await,
        "没有订单 zzzzzzzzzzzzzzzzzzzz。",
    );
    let (_, _, unpaid) = ordered(&app, "alice@qq.com", "pilotusera").await;
    refused(
        &approve(&mut op, &app, &unpaid, "checking").await,
        &format!("订单 {unpaid} 还没有付款。"),
    );
    // Paid, and its account unbound before its card was known: it waits
    // for its binding.
    let (_, bob, unbound) = ordered(&app, "bob@qq.com", "pilotuserb").await;
    deliver(&app, &["checkout-payment"], &unbound, 2).await;
    app.store(move |store| store.unbind(&bob)).await.unwrap();
    deliver(&app, &["charge-payment"], &unbound, 2).await;
    refused(
        &approve(&mut op, &app, &unbound, "binding").await,
        &format!("订单 {unbound} 的账号没有绑定在订单写的 Mooring 用户上：先绑定，再放行。"),
    );
    app.bind(bob, "pilotuserb").await;
    let granted = approve(&mut op, &app, &unbound, "binding").await;
    assert_eq!(granted.status, StatusCode::OK, "{}", granted.body);
    refused(
        &approve(&mut op, &app, &unbound, "binding").await,
        &format!("订单 {unbound} 已经记入清单，等 Mooring 开通。"),
    );
    // Refunded in full, it waits for no one.
    let (_, _, returned) = ordered(&app, "carol@qq.com", "pilotuserc").await;
    deliver_short(&app, &returned, 3).await;
    deliver(&app, &["charge-refunded-all"], &returned, 3).await;
    refused(
        &approve(&mut op, &app, &returned, "amount").await,
        &format!("订单 {returned} 已经全额退款或有争议，不能放行。"),
    );
}

#[tokio::test]
async fn what_the_host_lists_besides_is_said_and_only_a_stray_payment_counted() {
    let app = TestApp::shop();
    let (mut op, _) = operator(&app).await;
    // A refund of a payment the portal never heard of: the host lists it,
    // and nothing waits for it.
    deliver(&app, &["charge-refunded-part"], "zzzzzzzzzzzzzzzzzzzz", 8).await;
    let page = op.get(&app, "/operator/payments").await.body;
    assert!(page.contains("没有等待中的订单。"), "{page}");
    // Listed for reference, it is no caution.
    assert!(
        page.contains(
            "<p class=\"small muted\">还有 1 笔没有对应订单的退款或争议，在主机上运行 orders 能看到，这里没有要做的。</p>"
        ),
        "{page}"
    );
    assert!(!page.contains("不属于任何订单的付款"), "{page}");
    assert!(!page.contains("class=\"count\""), "{page}");
    // A payment that is no order's waits for the operator, on the host.
    deliver(&app, &["checkout-payment"], "zzzzzzzzzzzzzzzzzzzz", 9).await;
    let page = op.get(&app, "/operator/payments").await.body;
    assert!(page.contains("没有等待中的订单。"), "{page}");
    assert!(
        page.contains(
            "<p class=\"caution\">另有 1 笔不属于任何订单的付款：在主机上运行 orders 查看和处理。</p>"
        ),
        "{page}"
    );
    assert!(page.contains("还有 1 笔没有对应订单的退款或争议"), "{page}");
    assert!(
        page.contains("付款 <span class=\"count\">1</span>"),
        "{page}"
    );
}

#[tokio::test]
async fn an_order_disputed_before_it_was_granted_is_listed_apart_and_not_counted() {
    let app = TestApp::shop();
    let (mut op, _) = operator(&app).await;
    let (_, _, wallet) = ordered(&app, "alice@qq.com", "pilotusera").await;
    deliver(&app, &["checkout-payment", "charge-wallet"], &wallet, 1).await;
    let page = op.get(&app, "/operator/payments").await.body;
    assert!(page.contains(&form_of(&wallet, "card_unchecked")), "{page}");
    assert!(!page.contains("被争议的订单"), "{page}");
    deliver(&app, &["dispute-created"], &wallet, 1).await;
    let page = op.get(&app, "/operator/payments").await.body;
    let (waiting, rest) = page.split_once("<h2>被争议的订单</h2>").unwrap();
    let (disputed, latest) = rest.split_once("<h2>最近的订单</h2>").unwrap();
    assert!(waiting.contains("没有等待中的订单。"), "{page}");
    assert!(!waiting.contains(&wallet), "{page}");
    assert!(disputed.contains("门户不跟踪争议的结果"), "{page}");
    assert!(disputed.contains(&wallet), "{page}");
    assert!(
        disputed.contains(
            "付款人对这笔付款提出了争议，订单从没开通过：到 Stripe 后台回应争议，这里没有要做的。"
        ),
        "{page}"
    );
    assert!(disputed.contains("pi_3SynthPayment1"), "{page}");
    assert!(!page.contains("name=\"order\""), "{page}");
    assert!(latest.contains(&wallet), "{page}");
    // Nothing waits for the operator: no count in the bar, the tab or
    // the overview.
    assert!(!page.contains("class=\"count\""), "{page}");
    let overview = op.get(&app, "/operator").await.body;
    assert!(
        overview.contains("等你处理的付款</p>\n      <p class=\"value\">0</p>"),
        "{overview}"
    );
}
