//! Plans, ordering and an order's page.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE, ORIGIN};

use super::paying::FIXTURE_ORDER;
use super::support::{Browser, ORIGIN_URL, STRICT_POLICY, TestApp};

#[tokio::test]
async fn a_portal_without_a_catalogue_sells_nothing_and_says_so() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    let plans = browser.get(&app, "/plans").await.body;
    assert!(plans.contains("付款还没有开通"), "{plans}");
    assert!(!plans.contains("套餐与价格") && !plans.contains("£"));
    let subscription = browser.get(&app, "/subscription").await.body;
    assert!(!subscription.contains("购买与续费") && !subscription.contains("/subscription/orders"));
    // None of the payment addresses is there.
    let ordered = browser
        .post(&app, "/subscription/orders", &[("plan", "month-100")])
        .await;
    assert_eq!(ordered.status, StatusCode::NOT_FOUND);
    let page = browser.order_page(&app, FIXTURE_ORDER).await;
    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert_eq!(app.deliver("{}").await.status, StatusCode::NOT_FOUND);
    let pulled = app.pull("/api/mooring/entitlements?after=0", None).await;
    assert_eq!(pulled.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn where_nothing_is_sold_the_payment_addresses_are_like_any_that_never_was() {
    let app = TestApp::new();
    let (browser, _) = app.signed_up("alice@qq.com").await;
    // Whatever the method, and without the token a form would need: the
    // answer is the one an address that was never one gets, to the byte.
    for method in ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        let ask = |path: &str| {
            browser
                .request(method, path)
                .header(ORIGIN, ORIGIN_URL)
                .header("stripe-signature", "t=1,v1=00")
                .header("authorization", format!("Bearer {}", "0".repeat(64)))
                .body(Body::from("plan=month-100"))
                .unwrap()
        };
        let nowhere = app.send(ask("/no/such/address")).await;
        assert_eq!(nowhere.status, StatusCode::NOT_FOUND, "{method}");
        for path in [
            "/subscription/orders".to_owned(),
            format!("/subscription/orders/{FIXTURE_ORDER}"),
            "/stripe/webhook".to_owned(),
            "/api/mooring/entitlements".to_owned(),
            "/api/mooring/entitlements?after=0".to_owned(),
        ] {
            let reply = app.send(ask(&path)).await;
            assert_eq!(
                (reply.status, &reply.headers, &reply.body),
                (nowhere.status, &nowhere.headers, &nowhere.body),
                "{method} {path}"
            );
        }
    }
    // A portal that sells has them: asked the wrong way, they say so.
    let shop = TestApp::shop();
    for (method, path) in [
        ("GET", "/subscription/orders"),
        ("POST", "/subscription/orders/0123456789abcdefghjk"),
        ("GET", "/stripe/webhook"),
        ("POST", "/api/mooring/entitlements"),
    ] {
        let reply = shop
            .send(
                Browser::new([198, 51, 100, 9])
                    .request(method, path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn where_nothing_is_sold_nothing_is_said_of_ordering() {
    let ordering = [
        ("plan_gone", "这个套餐已经不再出售。"),
        ("order_unbound", "你的账号还没有绑定，暂时不能下单。"),
        ("order_renewing", "你已经有一个自动续费的套餐。"),
    ];
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    for (notice, said) in ordering {
        let page = browser
            .get(&app, &format!("/subscription?notice={notice}"))
            .await;
        assert_eq!(page.status, StatusCode::OK, "{notice}");
        assert!(!page.body.contains("role=\"status\""), "{notice}");
        assert!(!page.body.contains(said), "{notice}");
    }
    // What is said of the subscription link still is.
    let told = browser
        .get(&app, "/subscription?notice=link_revoked")
        .await
        .body;
    assert!(told.contains("<p class=\"notice\" role=\"status\">订阅链接已停用。</p>"));
    // A portal that sells says each.
    let shop = TestApp::shop();
    let (mut browser, _) = shop.signed_up("alice@qq.com").await;
    for (notice, said) in ordering {
        let page = browser
            .get(&shop, &format!("/subscription?notice={notice}"))
            .await
            .body;
        assert!(page.contains(said), "{notice}: {page}");
    }
}

#[tokio::test]
async fn the_plans_page_lists_what_is_on_sale_and_how_to_pay() {
    let app = TestApp::shop();
    let mut browser = Browser::new([198, 51, 100, 9]);
    let page = browser.get(&app, "/plans").await.body;
    for expected in [
        "套餐与价格",
        "月付",
        "30 天，流量 100 GB",
        "£8.00",
        "月付 · 自动续费",
        "每 30 天自动续费一次，每期流量 100 GB",
        "流量包",
        "90 天内有效，额外流量 50 GB",
        "£3.00",
        "去订阅页下单",
        "现在是 Stripe 的测试模式",
        "只接受银行卡，不接受支付宝和微信支付。你在 Stripe 的页面上付款",
    ] {
        assert!(page.contains(expected), "{expected}: {page}");
    }
    assert!(!page.contains("付款还没有开通"));
    // No form to order here: who orders is known only once signed in.
    assert!(
        !page.contains("<form class=\"inline\" method=\"post\" action=\"/subscription/orders\"")
    );
    let english = {
        browser
            .post(&app, "/lang", &[("to", "en"), ("back", "plans")])
            .await;
        browser.get(&app, "/plans").await.body
    };
    for expected in [
        "Plans and prices",
        "30 days with 100 GB of traffic",
        "Renews every 30 days, with 100 GB of traffic each time",
        "Valid for 90 days: 50 GB of extra traffic",
        "Stripe is in test mode",
    ] {
        assert!(english.contains(expected), "{expected}: {english}");
    }
}

#[tokio::test]
async fn only_a_bound_account_is_offered_the_shop() {
    let app = TestApp::shop();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    let unbound = browser.get(&app, "/subscription").await.body;
    assert!(!unbound.contains("购买与续费"), "{unbound}");
    // Asking anyway is answered, not served.
    let refused = browser
        .post(&app, "/subscription/orders", &[("plan", "month-100")])
        .await;
    assert_eq!(refused.status, StatusCode::SEE_OTHER);
    assert_eq!(refused.location(), "/subscription?notice=order_unbound");
    let told = browser.get(&app, refused.location()).await.body;
    assert!(
        told.contains("你的账号还没有绑定，暂时不能下单。"),
        "{told}"
    );
    app.bind(code, "pilotusera").await;
    let bound = browser.get(&app, "/subscription").await.body;
    for expected in [
        "购买与续费",
        "name=\"plan\" value=\"month-100\"",
        "name=\"plan\" value=\"month-100-auto\"",
        "name=\"plan\" value=\"pack-50\"",
        "£8.00",
        "£3.00",
    ] {
        assert!(bound.contains(expected), "{expected}: {bound}");
    }
    // Nothing ordered yet, nothing renewing.
    assert!(!bound.contains("我的订单") && !bound.contains("billing.stripe.com"));
    assert_eq!(bound.matches("action=\"/subscription/orders\"").count(), 3);
    // Each form to order carries the browser's token against forgery.
    let shop = bound
        .split("<section class=\"panel shop\">")
        .nth(1)
        .and_then(|rest| rest.split("</section>").next())
        .unwrap();
    let token = format!(
        "<input type=\"hidden\" name=\"csrf\" value=\"{}\">",
        browser.cookies["__Host-csrf"]
    );
    assert_eq!(shop.matches(&token).count(), 3, "{shop}");
}

#[tokio::test]
async fn an_order_is_placed_and_its_page_leads_to_stripe() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    let page = browser.order_page(&app, &id).await;
    assert_eq!(page.status, StatusCode::OK);
    for expected in [
        "待付款",
        "月付",
        "30 天，流量 100 GB",
        "£8.00",
        id.as_str(),
        "去 Stripe 付款",
        "不要用 Apple Pay",
        "现在是 Stripe 的测试模式",
    ] {
        assert!(page.body.contains(expected), "{expected}: {}", page.body);
    }
    // The way to Stripe is a link that names the order and the language,
    // and tells Stripe nothing of where it came from.
    let link = format!(
        "<a class=\"button\" href=\"https://buy.stripe.com/test_SynthMonth?client_reference_id={id}&#38;locale=zh\" rel=\"noopener noreferrer\">"
    );
    assert!(page.body.contains(&link), "{}", page.body);
    assert_eq!(page.headers[CONTENT_SECURITY_POLICY], STRICT_POLICY);
    // Switching the language would leave the order, so it is not offered.
    assert!(!page.body.contains("action=\"/lang\""));
    // Asked for again, the same order; another plan, another.
    assert_eq!(browser.order(&app, "month-100").await, id);
    let pack = browser.order(&app, "pack-50").await;
    assert_ne!(pack, id);
    let pack_page = browser.order_page(&app, &pack).await.body;
    assert!(pack_page.contains("https://buy.stripe.com/test_SynthPack?client_reference_id="));
    // The subscription page lists both, the newest first... they were
    // placed in the same second, so by their ids.
    let listed = browser.get(&app, "/subscription").await.body;
    assert!(listed.contains("我的订单"));
    for order in [&id, &pack] {
        assert!(
            listed.contains(&format!("href=\"/subscription/orders/{order}\"")),
            "{listed}"
        );
    }
    // In English, Stripe's page is asked for in English.
    browser
        .post(&app, "/lang", &[("to", "en"), ("back", "subscription")])
        .await;
    let english = browser.order_page(&app, &id).await.body;
    assert!(
        english.contains(&format!("client_reference_id={id}&#38;locale=en")),
        "{english}"
    );
    assert!(english.contains("Not paid") && english.contains("Pay at Stripe"));
}

#[tokio::test]
async fn an_order_needs_a_session_a_token_and_a_plan_on_sale() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    // A plan the catalogue does not have, whatever else the form holds.
    for plan in ["month-999", "", "MONTH-100", "month-100 "] {
        let gone = browser
            .post(
                &app,
                "/subscription/orders",
                &[("plan", plan), ("amount", "1")],
            )
            .await;
        assert_eq!(gone.status, StatusCode::SEE_OTHER, "{plan:?}");
        assert_eq!(
            gone.location(),
            "/subscription?notice=plan_gone",
            "{plan:?}"
        );
    }
    let told = browser
        .get(&app, "/subscription?notice=plan_gone")
        .await
        .body;
    assert!(told.contains("这个套餐已经不再出售。"));
    // No plan named at all.
    let unnamed = browser.post(&app, "/subscription/orders", &[]).await;
    assert_eq!(unnamed.status, StatusCode::BAD_REQUEST);
    // Without the CSRF token, or from another origin.
    let forged = browser
        .request("POST", "/subscription/orders")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ORIGIN, ORIGIN_URL)
        .body(Body::from("plan=month-100"))
        .unwrap();
    assert_eq!(app.send(forged).await.status, StatusCode::FORBIDDEN);
    let token = browser.cookies["__Host-csrf"].clone();
    let elsewhere = browser
        .request("POST", "/subscription/orders")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ORIGIN, "https://evil.example")
        .body(Body::from(format!("csrf={token}&plan=month-100")))
        .unwrap();
    assert_eq!(app.send(elsewhere).await.status, StatusCode::FORBIDDEN);
    // Signed out, the form leads to the login page.
    let mut stranger = Browser::new([203, 0, 113, 70]);
    stranger.get(&app, "/login").await;
    let signed_out = stranger
        .post(&app, "/subscription/orders", &[("plan", "month-100")])
        .await;
    assert_eq!(signed_out.status, StatusCode::SEE_OTHER);
    assert_eq!(signed_out.location(), "/login?next=subscription");
    // Nothing was ordered by any of it.
    let orders = app.store(|store| store.recent_orders(10)).await.unwrap();
    assert!(orders.is_empty(), "{orders:?}");
}

#[tokio::test]
async fn an_orders_page_is_its_owners_alone() {
    let app = TestApp::shop();
    let (mut owner, _) = app.customer("alice@qq.com", "pilotusera").await;
    let (mut other, _) = app.customer("bob@qq.com", "pilotuserb").await;
    let id = owner.order(&app, "month-100").await;
    assert_eq!(owner.order_page(&app, &id).await.status, StatusCode::OK);
    // Another account's request is answered as for no order at all.
    let theirs = other.order_page(&app, &id).await;
    let none = other.order_page(&app, FIXTURE_ORDER).await;
    assert_eq!(theirs.status, StatusCode::NOT_FOUND);
    assert_eq!(theirs.body, none.body);
    assert!(!theirs.body.contains(&id));
    // So is anything that is not an order's id.
    for path in [
        "/subscription/orders/",
        "/subscription/orders/short",
        "/subscription/orders/0123456789ABCDEFGHJK",
        "/subscription/orders/0123456789abcdefghjk/extra",
        "/subscription/orders/%ff",
    ] {
        assert_eq!(
            other.get(&app, path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    // Signed out, an order's address leads to the login page.
    let mut stranger = Browser::new([203, 0, 113, 71]);
    let signed_out = stranger.order_page(&app, &id).await;
    assert_eq!(signed_out.status, StatusCode::SEE_OTHER);
    assert_eq!(signed_out.location(), "/login?next=subscription");
    // And the other's list does not hold it.
    let listed = other.get(&app, "/subscription").await.body;
    assert!(!listed.contains(&id));
}

#[tokio::test]
async fn an_orders_page_follows_its_payment() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment"], &id).await;
    let checking = browser.order_page(&app, &id).await.body;
    assert!(checking.contains("已付款，正在核对"), "{checking}");
    // 2026-11-11T00:01:00Z, in Beijing.
    assert!(checking.contains("2026-11-11 08:01"), "{checking}");
    // Paid, there is nothing to pay.
    assert!(!checking.contains("buy.stripe.com") && !checking.contains("去 Stripe 付款"));
    app.deliver_events(&["charge-payment"], &id).await;
    let granted = browser.order_page(&app, &id).await.body;
    assert!(granted.contains("已付款，等待开通"), "{granted}");
    assert!(granted.contains("开通由运营者一侧的系统定时处理"));
    // Mooring fetches it, and says so with its next pull.
    let token = app.pull_token().await;
    let bearer = format!("Bearer {token}");
    app.pull("/api/mooring/entitlements?after=0", Some(&bearer))
        .await;
    assert!(
        browser
            .order_page(&app, &id)
            .await
            .body
            .contains("等待开通")
    );
    app.pull("/api/mooring/entitlements?after=1", Some(&bearer))
        .await;
    let pulled = browser.order_page(&app, &id).await.body;
    assert!(pulled.contains("已付款，已提交开通"), "{pulled}");
    assert!(pulled.contains("运营者一侧的系统已经取走这张订单"));
    // Part of it goes back, then all of it, then it is disputed.
    app.deliver_events(&["charge-refunded-part"], &id).await;
    assert!(
        browser
            .order_page(&app, &id)
            .await
            .body
            .contains("已退款 £3.00")
    );
    app.deliver_events(&["charge-refunded-all", "dispute-created"], &id)
        .await;
    let returned = browser.order_page(&app, &id).await.body;
    assert!(
        returned.contains("已全额退款") && returned.contains("提出了争议"),
        "{returned}"
    );
    assert!(!returned.contains("已退款 £"));
    let listed = browser.get(&app, "/subscription").await.body;
    assert!(listed.contains("已付款，已提交开通"), "{listed}");
}

#[tokio::test]
async fn a_payment_that_waits_for_the_operator_says_so_and_no_more() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment", "charge-wallet"], &id)
        .await;
    let page = browser.order_page(&app, &id).await.body;
    assert!(page.contains("需要运营者核对"), "{page}");
    assert!(page.contains("请联系运营者：核对后会开通或退款。"));
    // Why is the operator's to say.
    for inside in ["card_unchecked", "wallet", "钱包", "封禁"] {
        assert!(!page.contains(inside), "{inside}: {page}");
    }
    // A payment returned before it was granted.
    let pack = browser.order(&app, "pack-50").await;
    app.deliver_events(&["checkout-pack"], &pack).await;
    let refund = super::paying::event("charge-refunded-all", &pack)
        .replace("pi_3SynthPayment", "pi_3SynthPack");
    assert_eq!(app.deliver(&refund).await.status, StatusCode::OK);
    app.deliver_events(&["charge-pack"], &pack).await;
    let returned = browser.order_page(&app, &pack).await.body;
    assert!(
        returned.contains("付款已退回") && returned.contains("所以不会开通"),
        "{returned}"
    );
}

#[tokio::test]
async fn an_order_whose_account_is_no_longer_bound_cannot_be_paid() {
    let app = TestApp::shop();
    let (mut browser, code) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100").await;
    app.store(move |store| store.unbind(&code)).await.unwrap();
    let page = browser.order_page(&app, &id).await.body;
    assert!(page.contains("这张订单不能再付款"), "{page}");
    assert!(!page.contains("buy.stripe.com"));
    // Bound to another user, the same; ordering again makes a new order.
    app.bind(code, "pilotuserb").await;
    assert!(
        browser
            .order_page(&app, &id)
            .await
            .body
            .contains("这张订单不能再付款")
    );
    let fresh = browser.order(&app, "month-100").await;
    assert_ne!(fresh, id);
    assert!(
        browser
            .order_page(&app, &fresh)
            .await
            .body
            .contains("buy.stripe.com")
    );
    assert_eq!(
        browser.order_page(&app, &id).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_renewing_plan_is_shown_with_the_way_to_cancel_it_and_not_sold_twice() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    let id = browser.order(&app, "month-100-auto").await;
    let unpaid = browser.order_page(&app, &id).await.body;
    assert!(unpaid.contains("https://buy.stripe.com/test_SynthAuto?client_reference_id="));
    app.deliver_events(
        &[
            "checkout-subscription",
            "invoice-paid-first",
            "invoice-payment-first",
            "charge-first-invoice",
        ],
        &id,
    )
    .await;
    let page = browser.get(&app, "/subscription").await.body;
    for expected in [
        "你的自动续费正在进行",
        "href=\"https://billing.stripe.com/p/login/test_SynthManage\" rel=\"noopener noreferrer\"",
        "用付款时填写的邮箱登录",
        "你已经有一个自动续费的套餐",
        "已付款，等待开通",
    ] {
        assert!(page.contains(expected), "{expected}: {page}");
    }
    // The renewing plan has no form now; the others have.
    assert!(!page.contains("name=\"plan\" value=\"month-100-auto\""));
    assert_eq!(page.matches("action=\"/subscription/orders\"").count(), 2);
    let refused = browser
        .post(&app, "/subscription/orders", &[("plan", "month-100-auto")])
        .await;
    assert_eq!(refused.location(), "/subscription?notice=order_renewing");
    let told = browser.get(&app, refused.location()).await.body;
    assert!(
        told.contains("role=\"status\">你已经有一个自动续费的套餐"),
        "{told}"
    );
    // A renewal makes an order nobody placed, and its page says so.
    app.deliver_events(
        &[
            "invoice-paid-cycle",
            "invoice-payment-cycle",
            "charge-cycle-invoice",
        ],
        &id,
    )
    .await;
    let renewal = "sbc7dxq3cceajeb9ncge";
    let renewed = browser.order_page(&app, renewal).await.body;
    assert!(renewed.contains("这是自动续费产生的订单。"), "{renewed}");
    assert!(renewed.contains("已付款，等待开通"));
    // Cancelled at the period's end, then over.
    app.deliver_events(&["subscription-ending"], &id).await;
    let ending = browser.get(&app, "/subscription").await.body;
    assert!(
        ending.contains("你的自动续费已经取消：本期结束后不再扣款。"),
        "{ending}"
    );
    assert!(ending.contains("billing.stripe.com"));
    app.deliver_events(&["subscription-deleted"], &id).await;
    let over = browser.get(&app, "/subscription").await.body;
    assert!(!over.contains("billing.stripe.com") && !over.contains("自动续费正在进行"));
    assert!(over.contains("name=\"plan\" value=\"month-100-auto\""));
}

#[tokio::test]
async fn an_order_from_a_session_that_has_since_ended_leads_to_the_login_page() {
    let app = TestApp::shop();
    let (mut browser, _) = app.customer("alice@qq.com", "pilotusera").await;
    // The session ends elsewhere (a logout on another device, say) while
    // this browser still holds its cookie.
    let token = portal_core::auth::SecretToken::parse(&browser.cookies["__Host-session"])
        .unwrap()
        .digest();
    app.store(move |store| store.end_session(&token))
        .await
        .unwrap();
    let reply = browser
        .post(&app, "/subscription/orders", &[("plan", "month-100")])
        .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location(), "/login?next=subscription");
    assert!(
        app.store(|store| store.recent_orders(10))
            .await
            .unwrap()
            .is_empty()
    );
}
