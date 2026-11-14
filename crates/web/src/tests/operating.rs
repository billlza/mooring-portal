//! What the operator does on the operator pages, and what each page says
//! it did: answering feedback, binding, unbinding and banning accounts,
//! and closing bookings.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, ORIGIN};
use portal_core::account::AccountCode;

use super::operator::operator;
use super::support::{ORIGIN_URL, TestApp, cfm_document};

/// The bodies the conversation of `code` holds, oldest first.
async fn conversation(app: &TestApp, code: AccountCode) -> Vec<(bool, String)> {
    app.store(move |store| store.conversation_with(&code))
        .await
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|message| (message.from_operator, message.body.as_str().to_owned()))
        .collect()
}

/// The Mooring user the account with `code` is bound to, if it is there
/// and bound; `None` inside if it is there unbound.
pub(super) async fn binding(app: &TestApp, code: AccountCode) -> Option<Option<String>> {
    app.store(move |store| store.accounts())
        .await
        .unwrap()
        .into_iter()
        .find(|account| account.code == code)
        .map(|account| account.binding.map(|binding| binding.user.to_string()))
}

#[tokio::test]
async fn the_operator_answers_feedback_and_the_user_reads_it() {
    let app = TestApp::new();
    let (mut op, _) = operator(&app).await;
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (mut quiet, quiet_code) = app.signed_up("b@qq.com").await;
    user.get(&app, "/feedback").await;
    user.post(&app, "/feedback", &[("body", "<b>链接</b>打不开")])
        .await;
    // The navigation and the overview tell what waits.
    let overview = op.get(&app, "/operator").await;
    assert!(
        overview
            .body
            .contains("运营 <span class=\"count\">1</span></a>")
    );
    assert!(overview.body.contains("<p class=\"value tone-warn\">1</p>"));
    let list = op.get(&app, "/operator/feedback").await;
    assert!(list.body.contains(&format!(
        "<td class=\"mono\">{code}</td>\n            <td>a@qq.com</td>"
    )));
    assert!(list.body.contains("<span class=\"badge\">等你回复</span>"));
    assert!(!list.body.contains(&quiet_code.to_string()));

    let path = format!("/operator/feedback/{code}");
    let page = op.get(&app, &path).await;
    assert!(
        page.body
            .contains(&format!("<h2>与账号 {code} 的往来</h2>"))
    );
    assert!(page.body.contains("&#60;b&#62;链接&#60;/b&#62;打不开"));
    assert!(
        page.body
            .contains(&format!("class=\"message-who\">{code}</span>"))
    );
    let blank = op.post(&app, &path, &[("body", " \r\n ")]).await;
    assert_eq!(blank.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(blank.body.contains("请先写下回复。"));
    let replied = op
        .post(&app, &path, &[("body", "请在订阅页重新生成链接")])
        .await;
    assert_eq!(replied.status, StatusCode::SEE_OTHER, "{}", replied.body);
    assert_eq!(replied.location(), format!("{path}?notice=sent"));
    let shown = op.get(&app, replied.location()).await;
    assert!(shown.body.contains("回复已发送。"));
    assert!(
        shown
            .body
            .contains("<li class=\"message mine from-operator\">")
    );
    assert_eq!(
        conversation(&app, code).await,
        [
            (false, "<b>链接</b>打不开".to_owned()),
            (true, "请在订阅页重新生成链接".to_owned()),
        ]
    );
    // Nothing waits now.
    let overview = op.get(&app, "/operator").await;
    assert!(!overview.body.contains("class=\"count\""));
    assert!(
        op.get(&app, "/operator/feedback")
            .await
            .body
            .contains("<span class=\"muted\">已回复</span>")
    );
    assert!(
        user.get(&app, "/feedback")
            .await
            .body
            .contains("请在订阅页重新生成链接")
    );
    // The operator may write first, to an account that never wrote.
    let first = op
        .post(
            &app,
            &format!("/operator/feedback/{quiet_code}"),
            &[("body", "欢迎使用")],
        )
        .await;
    assert_eq!(first.status, StatusCode::SEE_OTHER);
    assert!(quiet.get(&app, "/account").await.body.contains("有新回复"));
}

#[tokio::test]
async fn an_address_that_names_no_account_has_no_conversation() {
    let app = TestApp::new();
    let (mut op, _) = operator(&app).await;
    for path in [
        "/operator/feedback/0000-0000-0009",
        "/operator/feedback/not-a-code",
        "/operator/feedback/0000-0000-0001/more",
    ] {
        assert_eq!(
            op.get(&app, path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let posted = op
        .post(
            &app,
            "/operator/feedback/0000-0000-0009",
            &[("body", "你好")],
        )
        .await;
    assert_eq!(posted.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn binding_says_what_it_did_or_why_it_did_nothing() {
    let app = TestApp::new();
    let (mut op, op_code) = operator(&app).await;
    let (_, alice) = app.signed_up("alice@qq.com").await;
    let (_, bob) = app.signed_up("bob@163.com").await;
    let page = op.get(&app, "/operator/accounts").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains(&format!(
        "<td class=\"mono\">{op_code} <span class=\"badge\">运营者</span></td>"
    )));
    // Every account but the operator's may be banned here.
    assert_eq!(
        page.body
            .matches("action=\"/operator/accounts/ban\"")
            .count(),
        2
    );
    // The operator's own account, unbound, has only the binding form;
    // another's has the ban form too.
    let forms_for = |code: &AccountCode| {
        page.body
            .matches(&format!(
                "<input type=\"hidden\" name=\"account\" value=\"{code}\">"
            ))
            .count()
    };
    assert_eq!(forms_for(&op_code), 1);
    assert_eq!(forms_for(&alice), 2);
    let alice_text = alice.to_string();
    let bob_text = bob.to_string();
    for (account, user, status, said) in [
        (
            alice_text.as_str(),
            "Not An Id!",
            StatusCode::UNPROCESSABLE_ENTITY,
            "<p class=\"alert\" role=\"alert\">没有改动：Mooring 用户编号的格式不对。</p>"
                .to_owned(),
        ),
        (
            alice_text.as_str(),
            " pilotusera ",
            StatusCode::OK,
            format!("<p class=\"notice\" role=\"status\">已绑定账号 {alice}。</p>"),
        ),
        (
            alice_text.as_str(),
            "pilotuserb",
            StatusCode::CONFLICT,
            format!("没有改动：账号 {alice} 已经绑定了 Mooring 用户 pilotusera，要换先解绑。"),
        ),
        (
            bob_text.as_str(),
            "pilotusera",
            StatusCode::CONFLICT,
            format!(
                "没有改动：Mooring 用户 pilotusera 已经绑定在账号 {alice} 上，先解绑那个账号。"
            ),
        ),
        (
            "0000-0000-0009",
            "pilotuserc",
            StatusCode::CONFLICT,
            "没有改动：没有账号 0000-0000-0009。".to_owned(),
        ),
    ] {
        let reply = op
            .post(
                &app,
                "/operator/accounts/bind",
                &[("account", account), ("user", user)],
            )
            .await;
        assert_eq!(reply.status, status, "{user}");
        assert!(reply.body.contains(&said), "{said}\n{}", reply.body);
        // The page lists the accounts as they are now.
        assert!(reply.body.contains("<td>bob@163.com</td>"), "{user}");
    }
    assert_eq!(
        binding(&app, alice).await,
        Some(Some("pilotusera".to_owned()))
    );
    assert_eq!(binding(&app, bob).await, Some(None));
    let malformed = op
        .post(
            &app,
            "/operator/accounts/bind",
            &[("account", "nope"), ("user", "pilotuserc")],
        )
        .await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unbinding_and_banning_say_what_stripe_and_mooring_still_hold() {
    let app = TestApp::shop();
    let (mut op, _) = operator(&app).await;
    let (mut alice_browser, alice) = app.customer("alice@qq.com", "pilotusera").await;
    let (_, bob) = app.customer("bob@163.com", "pilotuserb").await;
    let first = alice_browser.order(&app, "month-100-auto").await;
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
    let stripe = "Stripe 可能还会扣这个账号的订阅 sub_1SynthAuto 的款";

    let unbound = op
        .post(
            &app,
            "/operator/accounts/unbind",
            &[("account", &bob.to_string())],
        )
        .await;
    assert_eq!(unbound.status, StatusCode::OK);
    // The user it was bound to is named, so that a mistake can be undone.
    assert!(unbound.body.contains(&format!(
        "已解绑账号 {bob} 和 Mooring 用户 pilotuserb，账号的订阅记录、CFM 配置和订阅链接已删除。要恢复，把账号重新绑定到这个用户。"
    )));
    // Bob's account began no subscription.
    assert!(!unbound.body.contains("class=\"caution\""));
    let again = op
        .post(
            &app,
            "/operator/accounts/unbind",
            &[("account", &bob.to_string())],
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert!(
        again
            .body
            .contains(&format!("没有改动：账号 {bob} 没有绑定。"))
    );

    let unbound = op
        .post(
            &app,
            "/operator/accounts/unbind",
            &[("account", &alice.to_string())],
        )
        .await;
    assert_eq!(unbound.status, StatusCode::OK);
    assert!(unbound.body.contains(&format!(
        "<p class=\"caution\" role=\"alert\">{stripe}，而它已经没有 Mooring 用户可以开通。"
    )));
    assert_eq!(binding(&app, alice).await, Some(None));

    app.bind(alice, "pilotusera").await;
    let banned = op
        .post(
            &app,
            "/operator/accounts/ban",
            &[("account", &alice.to_string())],
        )
        .await;
    assert_eq!(banned.status, StatusCode::OK);
    assert!(
        banned
            .body
            .contains(&format!("已封禁账号 {alice}，它的数据已删除。"))
    );
    assert!(banned.body.contains(&format!(
        "<p class=\"caution\" role=\"alert\">{stripe}：请在 Stripe 后台取消这个订阅。</p>"
    )));
    assert!(banned.body.contains(
        "<p class=\"caution\" role=\"alert\">这个账号绑定过 Mooring 用户 pilotusera：请在 Mooring 里作废这个用户。</p>"
    ));
    assert_eq!(binding(&app, alice).await, None);
    assert!(!banned.body.contains("alice@qq.com"));
    // Her browser is signed out with her account gone.
    let page = alice_browser.get(&app, "/account").await;
    assert_eq!(page.location(), "/login?next=account");
    let gone = op
        .post(
            &app,
            "/operator/accounts/ban",
            &[("account", &alice.to_string())],
        )
        .await;
    assert_eq!(gone.status, StatusCode::CONFLICT);
    assert!(gone.body.contains(&format!("没有改动：没有账号 {alice}。")));
    let gone = op
        .post(
            &app,
            "/operator/accounts/unbind",
            &[("account", &alice.to_string())],
        )
        .await;
    assert_eq!(gone.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_operators_account_is_not_banned_here() {
    let app = TestApp::new();
    let (mut op, op_code) = operator(&app).await;
    let refused = op
        .post(
            &app,
            "/operator/accounts/ban",
            &[("account", &op_code.to_string())],
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert!(refused.body.contains(
        "没有改动：运营者的账号不能在网页上封禁：先在主机上用 revoke-operator 取消它的运营权限。"
    ));
    assert_eq!(binding(&app, op_code).await, Some(None));
    assert_eq!(op.get(&app, "/operator").await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_form_from_elsewhere_changes_nothing() {
    let app = TestApp::new();
    let (op, _) = operator(&app).await;
    let (_, alice) = app.signed_up("alice@qq.com").await;
    // The operator's browser, with its cookies, sent a form another site
    // made: it cannot know the CSRF token.
    let forged = app
        .send(
            op.request("POST", "/operator/accounts/ban")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, ORIGIN_URL)
                .header("sec-fetch-site", "same-origin")
                .body(Body::from(format!("csrf=forged&account={alice}")))
                .unwrap(),
        )
        .await;
    assert_eq!(forged.status, StatusCode::FORBIDDEN);
    assert_eq!(binding(&app, alice).await, Some(None));
}

#[tokio::test]
async fn closing_a_booking_deletes_it() {
    let app = TestApp::new();
    let (mut op, _) = operator(&app).await;
    let empty = op.get(&app, "/operator/bookings").await;
    assert!(empty.body.contains("没有待处理的 Mac 预订。"));
    let (mut user, code) = app.signed_up("alice@qq.com").await;
    user.get(&app, "/mac").await;
    let booked = user
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "month")],
        )
        .await;
    assert_eq!(booked.status, StatusCode::SEE_OTHER);
    let id = app.store(|store| store.bookings()).await.unwrap()[0]
        .booking
        .id;
    let page = op.get(&app, "/operator/bookings").await;
    assert!(
        page.body
            .contains(&format!("<td class=\"mono\">{code}</td>"))
    );
    assert!(page.body.contains("<td>alice@qq.com</td>"));
    assert!(
        page.body
            .contains("<td class=\"when\">2026-11-05 20:00</td>")
    );
    assert!(page.body.contains(&format!(
        "<input type=\"hidden\" name=\"id\" value=\"{id}\">"
    )));
    let id_text = id.to_string();
    let closed = op
        .post(&app, "/operator/bookings/close", &[("id", &id_text)])
        .await;
    assert_eq!(closed.status, StatusCode::OK);
    assert!(closed.body.contains(&format!("已关闭预订 {id}。")));
    assert!(closed.body.contains("没有待处理的 Mac 预订。"));
    assert!(
        app.store(|store| store.bookings())
            .await
            .unwrap()
            .is_empty()
    );
    let again = op
        .post(&app, "/operator/bookings/close", &[("id", &id_text)])
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert!(
        again
            .body
            .contains(&format!("没有改动：没有编号为 {id} 的预订。"))
    );
    let malformed = op
        .post(&app, "/operator/bookings/close", &[("id", "first")])
        .await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_operator_pages_show_no_profile_link_or_password() {
    let app = TestApp::new();
    let (mut op, _) = operator(&app).await;
    let (mut user, code) = app.customer("alice@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    user.get(&app, "/subscription").await;
    // The link is shown once, to its owner, as it is made.
    let made = user.post(&app, "/subscription/link", &[]).await;
    assert!(made.body.contains("/sub/"), "{}", made.body);
    user.post(&app, "/feedback", &[("body", "你好")]).await;
    let secret = cfm_document("pilotusera");
    let credential = secret.split("\"uuid\": \"").nth(1).unwrap();
    let conversation = format!("/operator/feedback/{code}");
    for path in [
        "/operator",
        "/operator/feedback",
        conversation.as_str(),
        "/operator/accounts",
        "/operator/bookings",
        "/operator/payments",
    ] {
        let page = op.get(&app, path).await;
        assert_eq!(page.status, StatusCode::OK, "{path}");
        for hidden in [credential, "$argon2", "/sub/"] {
            assert!(!page.body.contains(hidden), "{path} shows {hidden}");
        }
    }
}

#[tokio::test]
async fn an_operator_reading_a_conversation_leaves_its_reply_unread() {
    let app = TestApp::new();
    let (mut op, _) = operator(&app).await;
    let (mut user, code) = app.signed_up("alice@qq.com").await;
    let path = format!("/operator/feedback/{code}");
    op.post(&app, &path, &[("body", "你好")]).await;
    op.get(&app, &path).await;
    assert!(user.get(&app, "/account").await.body.contains("有新回复"));
}
