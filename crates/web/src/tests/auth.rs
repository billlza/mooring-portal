//! Registration, login, logout and password changes.

use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;

use super::support::{Browser, PASSWORD, TestApp};
use crate::config::ClientIpSource;
use crate::security::limits::{
    LOGIN_PER_NETWORK, PASSWORD_CHECKS_PER_EMAIL, REGISTRATIONS_PER_NETWORK,
};

const NEW_PASSWORD: &str = "a whole new passphrase";

#[tokio::test]
async fn registering_logs_in_and_shows_the_account_code() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("LiLei@QQ.com").await;
    let page = browser.get(&app, "/account?notice=welcome").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains(&code.to_string()));
    assert!(page.body.contains("lilei@qq.com"));
    assert!(page.body.contains("注册成功"));
    assert!(page.body.contains("未绑定"));
    assert!(page.body.contains("我的账户"), "the pill names the account");
    let session = browser.cookies.get("__Host-session").unwrap();
    assert_eq!(session.len(), 64);
}

#[tokio::test]
async fn registration_errors_are_shown_on_the_form() {
    let app = TestApp::new();
    app.signed_up("lilei@qq.com").await;
    let mut browser = Browser::new([203, 0, 113, 2]);
    browser.get(&app, "/register").await;
    for (email, password, confirm, status, message) in [
        (
            "not an email",
            PASSWORD,
            PASSWORD,
            StatusCode::UNPROCESSABLE_ENTITY,
            "请输入有效的邮箱地址。",
        ),
        (
            "hmm@qq.com",
            "short",
            "short",
            StatusCode::UNPROCESSABLE_ENTITY,
            "密码至少需要 12 个字符。",
        ),
        (
            "hmm@qq.com",
            &"x".repeat(129),
            &"x".repeat(129),
            StatusCode::UNPROCESSABLE_ENTITY,
            "密码不能超过 128 个字符。",
        ),
        (
            "hmm@qq.com",
            "line\nbreak password",
            "line\nbreak password",
            StatusCode::UNPROCESSABLE_ENTITY,
            "控制字符",
        ),
        (
            "hmm.hmm.hmm@qq.com",
            "hmm.hmm.hmm@qq.com",
            "hmm.hmm.hmm@qq.com",
            StatusCode::UNPROCESSABLE_ENTITY,
            "不能与邮箱相同",
        ),
        (
            "hmm@qq.com",
            PASSWORD,
            "something else entirely",
            StatusCode::UNPROCESSABLE_ENTITY,
            "两次输入的密码不一致。",
        ),
        (
            "LILEI@qq.com",
            PASSWORD,
            PASSWORD,
            StatusCode::CONFLICT,
            "这个邮箱不能再注册。已经有账号的话，请直接登录。",
        ),
    ] {
        let reply = browser
            .post(
                &app,
                "/register",
                &[
                    ("email", email),
                    ("password", password),
                    ("confirm", confirm),
                ],
            )
            .await;
        assert_eq!(reply.status, status, "{email}");
        assert!(reply.body.contains(message), "{email}: {}", reply.body);
        assert!(reply.headers.get(SET_COOKIE).is_none());
        if password != email {
            assert!(
                !reply.body.contains(password),
                "a password is never sent back"
            );
        }
    }
    // The typed email is kept, escaped.
    let reply = browser
        .post(
            &app,
            "/register",
            &[("email", "<b>@x"), ("password", "x"), ("confirm", "x")],
        )
        .await;
    assert!(
        reply.body.contains("value=\"&#60;b&#62;@x\""),
        "{}",
        reply.body
    );
}

#[tokio::test]
async fn registration_closes_at_the_account_limit() {
    let app = TestApp::with(ClientIpSource::Peer, 1);
    app.signed_up("first@qq.com").await;
    let mut browser = Browser::new([203, 0, 113, 3]);
    browser.get(&app, "/register").await;
    let reply = browser
        .post(
            &app,
            "/register",
            &[
                ("email", "second@qq.com"),
                ("password", PASSWORD),
                ("confirm", PASSWORD),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert!(reply.body.contains("注册名额已满"));
}

#[tokio::test]
async fn registrations_are_limited_per_network() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 4]);
    browser.get(&app, "/register").await;
    for attempt in 0..REGISTRATIONS_PER_NETWORK.limit {
        let reply = browser
            .post(
                &app,
                "/register",
                &[("email", "bad"), ("password", "x"), ("confirm", "x")],
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{attempt}");
    }
    let reply = browser
        .post(
            &app,
            "/register",
            &[
                ("email", "ok@qq.com"),
                ("password", PASSWORD),
                ("confirm", PASSWORD),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(reply.body.contains("尝试次数过多，请 60 分钟后再试。"));
}

#[tokio::test]
async fn login_works_with_any_case_of_email_and_goes_where_asked() {
    let app = TestApp::new();
    app.signed_up("lilei@qq.com").await;
    let mut browser = Browser::new([203, 0, 113, 5]);
    browser.get(&app, "/login?next=subscription").await;
    let reply = browser
        .post(
            &app,
            "/login",
            &[
                ("email", " LiLei@QQ.COM "),
                ("password", PASSWORD),
                ("next", "subscription"),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location(), "/subscription");
    let cookies: Vec<&str> = reply
        .headers
        .get_all(SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect();
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with("__Host-session=") && c.contains("Max-Age=2592000"))
    );
    assert!(
        cookies.iter().any(|c| c.starts_with("__Host-csrf=")),
        "the CSRF token is rotated"
    );
    assert_eq!(browser.get(&app, "/login").await.location(), "/account");
    assert_eq!(browser.get(&app, "/register").await.location(), "/account");
}

#[tokio::test]
async fn failed_logins_do_not_say_whether_the_account_exists() {
    let app = TestApp::new();
    app.signed_up("lilei@qq.com").await;
    let mut browser = Browser::new([203, 0, 113, 6]);
    browser.get(&app, "/login").await;
    let mut bodies = Vec::new();
    for (email, password) in [
        ("lilei@qq.com", "wrong password here"),
        ("nobody@qq.com", "wrong password here"),
        ("not an email", "wrong password here"),
    ] {
        let reply = browser
            .post(&app, "/login", &[("email", email), ("password", password)])
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{email}");
        assert!(reply.body.contains("邮箱或密码不正确。"));
        assert!(reply.headers.get(SET_COOKIE).is_none());
        bodies.push(reply.body.replace(email, "EMAIL"));
    }
    assert_eq!(
        bodies[0], bodies[1],
        "an existing and a missing account read the same"
    );
}

#[tokio::test]
async fn a_banned_address_is_signed_out_and_refused_as_a_taken_one() {
    let app = TestApp::new();
    let (mut banned, code) = app.signed_up("lilei@qq.com").await;
    app.signed_up("hanmeimei@qq.com").await;
    let now = portal_core::time::UnixTime::from_secs(super::support::NOW).unwrap();
    app.store(move |store| store.ban(&code, now)).await.unwrap();
    // The banned browser's session is gone, and so is the login.
    assert_eq!(
        banned.get(&app, "/account").await.location(),
        "/login?next=account"
    );
    let mut browser = Browser::new([203, 0, 113, 6]);
    browser.get(&app, "/login").await;
    let login = browser
        .post(
            &app,
            "/login",
            &[("email", "lilei@qq.com"), ("password", PASSWORD)],
        )
        .await;
    assert_eq!(login.status, StatusCode::UNAUTHORIZED);
    assert!(login.body.contains("邮箱或密码不正确。"));
    // Registering the address again reads exactly as for a taken address.
    browser.get(&app, "/register").await;
    let mut bodies = Vec::new();
    for address in ["LiLei@QQ.com", "hanmeimei@qq.com"] {
        let reply = browser
            .post(
                &app,
                "/register",
                &[
                    ("email", address),
                    ("password", PASSWORD),
                    ("confirm", PASSWORD),
                ],
            )
            .await;
        assert_eq!(reply.status, StatusCode::CONFLICT, "{address}");
        assert!(
            reply
                .body
                .contains("这个邮箱不能再注册。已经有账号的话，请直接登录。"),
            "{address}"
        );
        bodies.push(reply.body.replace(address, "EMAIL"));
    }
    assert_eq!(
        bodies[0], bodies[1],
        "a banned and a taken address read alike"
    );
}

#[tokio::test]
async fn failures_lock_an_email_for_a_while_and_success_resets_them() {
    let app = TestApp::new();
    app.signed_up("lilei@qq.com").await;
    // Spread over networks, so only the per-email limit applies.
    for attempt in 0..PASSWORD_CHECKS_PER_EMAIL.limit {
        let mut browser = Browser::new([10, 0, 0, u8::try_from(attempt).unwrap()]);
        browser.get(&app, "/login").await;
        let reply = browser
            .post(
                &app,
                "/login",
                &[
                    ("email", "lilei@qq.com"),
                    ("password", "wrong password here"),
                ],
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{attempt}");
    }
    let mut browser = Browser::new([10, 0, 1, 1]);
    let reply = browser.log_in(&app, "lilei@qq.com").await;
    assert_eq!(
        reply.status,
        StatusCode::TOO_MANY_REQUESTS,
        "even the right password waits"
    );
    assert!(reply.body.contains("15 分钟后再试"));
    app.clock.advance(15 * 60).unwrap();
    let reply = browser.log_in(&app, "lilei@qq.com").await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn logins_are_limited_per_network() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 8]);
    browser.get(&app, "/login").await;
    for attempt in 0..LOGIN_PER_NETWORK.limit {
        let email = format!("user{attempt}@qq.com");
        let reply = browser
            .post(
                &app,
                "/login",
                &[("email", &email), ("password", "nope nope nope")],
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{attempt}");
    }
    let reply = browser
        .post(
            &app,
            "/login",
            &[("email", "x@qq.com"), ("password", "nope")],
        )
        .await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn logging_out_ends_the_session() {
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("lilei@qq.com").await;
    let stolen = browser.clone();
    let reply = browser.post(&app, "/logout", &[]).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location(), "/");
    assert!(!browser.cookies.contains_key("__Host-session"));
    assert_eq!(
        browser.get(&app, "/account").await.status,
        StatusCode::SEE_OTHER
    );
    // A copy of the old cookie no longer works either.
    let mut copy = stolen;
    assert_eq!(
        copy.get(&app, "/account").await.location(),
        "/login?next=account"
    );
    // Logging out without a session is harmless.
    let mut anonymous = Browser::new([203, 0, 113, 9]);
    anonymous.get(&app, "/").await;
    assert_eq!(
        anonymous.post(&app, "/logout", &[]).await.status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn changing_the_password_logs_out_other_browsers() {
    let app = TestApp::new();
    let (mut first, _) = app.signed_up("lilei@qq.com").await;
    let mut second = Browser::new([203, 0, 113, 10]);
    assert_eq!(
        second.log_in(&app, "lilei@qq.com").await.status,
        StatusCode::SEE_OTHER
    );
    let page = first.get(&app, "/account/password").await;
    assert!(page.body.contains("修改密码"));
    let wrong = first
        .post(
            &app,
            "/account/password",
            &[
                ("current", "not my password"),
                ("password", NEW_PASSWORD),
                ("confirm", NEW_PASSWORD),
            ],
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(wrong.body.contains("当前密码不正确。"));
    let weak = first
        .post(
            &app,
            "/account/password",
            &[
                ("current", PASSWORD),
                ("password", "short"),
                ("confirm", "short"),
            ],
        )
        .await;
    assert_eq!(weak.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mismatch = first
        .post(
            &app,
            "/account/password",
            &[
                ("current", PASSWORD),
                ("password", NEW_PASSWORD),
                ("confirm", "different still"),
            ],
        )
        .await;
    assert_eq!(mismatch.status, StatusCode::UNPROCESSABLE_ENTITY);
    let done = first
        .post(
            &app,
            "/account/password",
            &[
                ("current", PASSWORD),
                ("password", NEW_PASSWORD),
                ("confirm", NEW_PASSWORD),
            ],
        )
        .await;
    assert_eq!(done.status, StatusCode::SEE_OTHER);
    assert_eq!(done.location(), "/account?notice=password_changed");
    assert!(
        first
            .get(&app, "/account?notice=password_changed")
            .await
            .body
            .contains("密码已修改")
    );
    assert_eq!(
        second.get(&app, "/account").await.status,
        StatusCode::SEE_OTHER,
        "logged out"
    );
    let mut third = Browser::new([203, 0, 113, 11]);
    assert_eq!(
        third.log_in(&app, "lilei@qq.com").await.status,
        StatusCode::UNAUTHORIZED
    );
    third.get(&app, "/login").await;
    let reply = third
        .post(
            &app,
            "/login",
            &[("email", "lilei@qq.com"), ("password", NEW_PASSWORD)],
        )
        .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn wrong_current_passwords_count_toward_the_lock() {
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("lilei@qq.com").await;
    let fields = [
        ("current", "not my password"),
        ("password", NEW_PASSWORD),
        ("confirm", NEW_PASSWORD),
    ];
    for _ in 1..PASSWORD_CHECKS_PER_EMAIL.limit {
        let reply = browser.post(&app, "/account/password", &fields).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    }
    let last = browser.post(&app, "/account/password", &fields).await;
    assert_eq!(last.status, StatusCode::UNAUTHORIZED);
    let locked = browser.post(&app, "/account/password", &fields).await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(locked.body.contains("分钟后再试"));
    // Signed out, the password page sends the browser to log in.
    let mut anonymous = Browser::new([203, 0, 113, 12]);
    anonymous.get(&app, "/").await;
    let reply = anonymous.post(&app, "/account/password", &fields).await;
    assert_eq!(reply.location(), "/login?next=password");
}
