//! An operator's form whose body is still arriving when its session may no
//! longer act on the operator pages: the operator is revoked, logs out,
//! changes the password, or lets the confirmation run out. Nothing may be
//! written, and nothing only an operator may see is shown, not even on
//! the page that refuses a form: no account's email, no conversation, no
//! order. The session is checked when the request's
//! headers arrive, again once its body has, and again in the transaction
//! that writes.

use axum::http::StatusCode;
use portal_core::account::AccountCode;
use portal_store::{FeedbackId, OPERATOR_CONFIRM_SECS};

use super::operator::operator;
use super::support::{Browser, PASSWORD, TestApp};

/// What stops the operator's session from acting while a form's body is
/// held back.
#[derive(Debug, Clone, Copy)]
enum Stop {
    Revoked,
    LoggedOut,
    NewPassword,
    Expired,
}

/// The forms of the operator pages: those that write, those whose fields
/// the page refuses by showing what only an operator may see, and one it
/// refuses before asking the database anything.
#[derive(Debug, Clone, Copy)]
enum Form {
    Bind,
    /// A Mooring user id that is none: refused with every account.
    BindBadUser,
    Unbind,
    Ban,
    Close,
    Reply,
    /// An empty reply: refused with the conversation.
    ReplyBlank,
    Approve,
    /// An order there is none of: refused with every order that waits.
    ApproveUnknown,
    /// No order's id at all: refused as no page's form, whose refusal
    /// would tell a session that may not act that the pages are there.
    ApproveMalformed,
}

const FORMS: [Form; 10] = [
    Form::Bind,
    Form::BindBadUser,
    Form::Unbind,
    Form::Ban,
    Form::Close,
    Form::Reply,
    Form::ReplyBlank,
    Form::Approve,
    Form::ApproveUnknown,
    Form::ApproveMalformed,
];

/// What only an operator may see of the target: its email, and what it
/// wrote to the operator.
const TARGET_EMAIL: &str = "target@qq.com";
const TARGET_WROTE: &str = "只给运营者看的话";

/// What a form changes: who the target is bound to, whether it is there,
/// how many bookings there are, how many messages its conversation holds,
/// and how many orders wait.
async fn state_of(
    app: &TestApp,
    target: AccountCode,
) -> (Option<String>, bool, usize, usize, usize) {
    app.store(move |store| {
        let account = store
            .accounts()
            .unwrap()
            .into_iter()
            .find(|account| account.code == target);
        let messages = store
            .conversation_with(&target, FeedbackId::START)
            .unwrap()
            .map_or(0, |messages| messages.len());
        (
            account
                .as_ref()
                .and_then(|account| account.binding.as_ref())
                .map(|binding| binding.user.to_string()),
            account.is_some(),
            store.bookings().unwrap().len(),
            messages,
            store.orders_waiting().unwrap().len(),
        )
    })
    .await
}

/// Holds `form` back from a confirmed operator, lets `stop` happen, then
/// lets the body come: nothing changes, and the reply turns the request
/// away as the gate would have.
async fn held_while(form: Form, stop: Stop) {
    let app = TestApp::shop();
    let (mut op, op_code) = operator(&app).await;
    let (mut target_browser, target) = app.customer(TARGET_EMAIL, "pilotusera").await;
    target_browser
        .post(&app, "/feedback", &[("body", TARGET_WROTE)])
        .await;
    // Paid, and waiting for word of its card.
    let order = target_browser.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment"], &order).await;
    target_browser.get(&app, "/mac").await;
    target_browser
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "month")],
        )
        .await;
    if matches!(form, Form::Bind) {
        app.store(move |store| store.unbind(&target)).await.unwrap();
    }
    let code = target.to_string();
    let conversation = format!("/operator/feedback/{code}");
    let (path, fields, section): (&str, Vec<(&str, &str)>, &str) = match form {
        Form::Bind => (
            "/operator/accounts/bind",
            vec![("account", code.as_str()), ("user", "pilotuserz")],
            "accounts",
        ),
        Form::BindBadUser => (
            "/operator/accounts/bind",
            vec![("account", code.as_str()), ("user", "no user")],
            "accounts",
        ),
        Form::Unbind => (
            "/operator/accounts/unbind",
            vec![("account", code.as_str())],
            "accounts",
        ),
        Form::Ban => (
            "/operator/accounts/ban",
            vec![("account", code.as_str())],
            "accounts",
        ),
        Form::Close => ("/operator/bookings/close", vec![("id", "1")], "bookings"),
        Form::Reply => (&conversation, vec![("body", "你好")], "feedback"),
        Form::ReplyBlank => (&conversation, vec![("body", "  ")], "feedback"),
        Form::Approve => (
            "/operator/payments/approve",
            vec![("order", order.as_str()), ("shown", "checking")],
            "payments",
        ),
        Form::ApproveUnknown => (
            "/operator/payments/approve",
            vec![("order", "zzzzzzzzzzzzzzzzzzzz"), ("shown", "checking")],
            "payments",
        ),
        Form::ApproveMalformed => (
            "/operator/payments/approve",
            vec![("order", "no order"), ("shown", "checking")],
            "payments",
        ),
    };
    let before = state_of(&app, target).await;
    let (release, pending) = op.held_post(&app, path, &fields).await;
    match stop {
        Stop::Revoked => {
            app.store(move |store| store.revoke_operator(&op_code))
                .await
                .unwrap();
        }
        Stop::LoggedOut => {
            op.post(&app, "/logout", &[]).await;
        }
        Stop::NewPassword => {
            let new_password = "a whole new passphrase";
            let changed = op
                .post(
                    &app,
                    "/account/password",
                    &[
                        ("current", PASSWORD),
                        ("password", new_password),
                        ("confirm", new_password),
                    ],
                )
                .await;
            assert_eq!(changed.status, StatusCode::SEE_OTHER, "{}", changed.body);
        }
        Stop::Expired => app.clock.advance(OPERATOR_CONFIRM_SECS).unwrap(),
    }
    release.send(()).unwrap();
    let reply = pending.await.unwrap();
    let case = format!("{form:?} after {stop:?}");
    match stop {
        Stop::Revoked => assert_eq!(reply.status, StatusCode::NOT_FOUND, "{case}"),
        Stop::LoggedOut | Stop::NewPassword => {
            assert_eq!(reply.location(), "/login?next=operator", "{case}");
        }
        Stop::Expired => {
            assert_eq!(
                reply.location(),
                format!("/operator/confirm?next={section}"),
                "{case}"
            );
        }
    }
    assert_eq!(state_of(&app, target).await, before, "{case}");
    for secret in [TARGET_EMAIL, TARGET_WROTE, order.as_str()] {
        assert!(!reply.body.contains(secret), "{case}: {}", reply.body);
    }
}

#[tokio::test]
async fn a_refused_form_shows_what_only_an_operator_may_see() {
    // What the held-back forms must not show, the operator's own forms do.
    let app = TestApp::shop();
    let (mut op, _) = operator(&app).await;
    let (mut target_browser, target) = app.customer(TARGET_EMAIL, "pilotusera").await;
    target_browser
        .post(&app, "/feedback", &[("body", TARGET_WROTE)])
        .await;
    let order = target_browser.order(&app, "month-100").await;
    app.deliver_events(&["checkout-payment"], &order).await;
    let code = target.to_string();
    let bad_user = op
        .post(
            &app,
            "/operator/accounts/bind",
            &[("account", &code), ("user", "no user")],
        )
        .await;
    assert_eq!(bad_user.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(bad_user.body.contains(TARGET_EMAIL), "{}", bad_user.body);
    let blank = op
        .post(
            &app,
            &format!("/operator/feedback/{code}"),
            &[("body", "  ")],
        )
        .await;
    assert_eq!(blank.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(blank.body.contains(TARGET_WROTE), "{}", blank.body);
    let unknown = op
        .post(
            &app,
            "/operator/payments/approve",
            &[("order", "zzzzzzzzzzzzzzzzzzzz"), ("shown", "checking")],
        )
        .await;
    assert_eq!(unknown.status, StatusCode::CONFLICT);
    assert!(unknown.body.contains(&order), "{}", unknown.body);
}

#[tokio::test]
async fn a_revoked_operators_held_form_writes_nothing() {
    for form in FORMS {
        held_while(form, Stop::Revoked).await;
    }
}

#[tokio::test]
async fn a_logged_out_operators_held_form_writes_nothing() {
    for form in FORMS {
        held_while(form, Stop::LoggedOut).await;
    }
}

#[tokio::test]
async fn a_held_form_from_before_a_new_password_writes_nothing() {
    for form in FORMS {
        held_while(form, Stop::NewPassword).await;
    }
}

#[tokio::test]
async fn a_held_form_whose_confirmation_runs_out_writes_nothing() {
    for form in FORMS {
        held_while(form, Stop::Expired).await;
    }
}

#[tokio::test]
async fn confirming_gives_the_browser_a_new_session_and_an_old_copy_nothing() {
    let app = TestApp::new();
    let (mut op, code) = app.signed_up("op@qq.com").await;
    super::operator::grant(&app, code).await;
    // Someone copied the operator's cookies before the confirmation.
    let mut copy: Browser = op.clone();
    let confirmed = super::operator::confirm(&mut op, &app, PASSWORD, "overview").await;
    assert_eq!(
        confirmed.status,
        StatusCode::SEE_OTHER,
        "{}",
        confirmed.body
    );
    for cookie in ["__Host-session", "__Host-csrf"] {
        assert_ne!(op.cookies[cookie], copy.cookies[cookie], "{cookie}");
    }
    assert_eq!(op.get(&app, "/operator").await.status, StatusCode::OK);
    // The copy's session was the one confirmed, and is gone.
    assert_eq!(
        copy.get(&app, "/operator").await.location(),
        "/login?next=operator"
    );
    assert_eq!(
        copy.get(&app, "/account").await.location(),
        "/login?next=account"
    );
    // The operator's form, made with the new cookies, goes through.
    let target = app.signed_up("a@qq.com").await.1.to_string();
    let bound = op
        .post(
            &app,
            "/operator/accounts/bind",
            &[("account", &target), ("user", "pilotusera")],
        )
        .await;
    assert_eq!(bound.status, StatusCode::OK, "{}", bound.body);
}

#[tokio::test]
async fn a_form_from_a_session_not_confirmed_lately_writes_nothing() {
    let app = TestApp::new();
    let (mut op, code) = app.signed_up("op@qq.com").await;
    super::operator::grant(&app, code).await;
    let (_, target) = app.signed_up("target@qq.com").await;
    app.bind(target, "pilotusera").await;
    for round in ["never confirmed", "confirmed too long ago"] {
        if round == "confirmed too long ago" {
            super::operator::confirm(&mut op, &app, PASSWORD, "overview").await;
            app.clock.advance(OPERATOR_CONFIRM_SECS).unwrap();
        }
        let before = state_of(&app, target).await;
        let code = target.to_string();
        let conversation = format!("/operator/feedback/{code}");
        for (path, fields, section) in [
            (
                "/operator/accounts/bind",
                vec![("account", code.as_str()), ("user", "pilotuserz")],
                "accounts",
            ),
            (
                "/operator/accounts/unbind",
                vec![("account", code.as_str())],
                "accounts",
            ),
            (
                "/operator/accounts/ban",
                vec![("account", code.as_str())],
                "accounts",
            ),
            ("/operator/bookings/close", vec![("id", "1")], "bookings"),
            (conversation.as_str(), vec![("body", "你好")], "feedback"),
            (
                "/operator/payments/approve",
                vec![("order", "0123456789abcdefghjk"), ("shown", "checking")],
                "payments",
            ),
        ] {
            let reply = op.post(&app, path, &fields).await;
            assert_eq!(
                reply.location(),
                format!("/operator/confirm?next={section}"),
                "{round}: {path}"
            );
        }
        assert_eq!(state_of(&app, target).await, before, "{round}");
    }
}
