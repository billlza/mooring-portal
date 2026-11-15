//! The pages that run the chat script: those that show a conversation and
//! hold no credential, under a policy that lets the script reach the
//! conversations and nothing else.

use axum::http::StatusCode;
use axum::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE};

use super::operator::operator;
use super::support::{ORIGIN_URL, STRICT_POLICY, TestApp};

/// The policy of a page that runs the chat script: it reaches the
/// conversations' addresses and nothing else.
fn chat_policy() -> String {
    format!(
        "{STRICT_POLICY}; script-src 'self'; connect-src {ORIGIN_URL}/feedback/ \
         {ORIGIN_URL}/operator/feedback/; worker-src 'none'"
    )
}

#[tokio::test]
async fn the_pages_that_run_the_chat_are_those_that_show_it_and_hold_no_credential() {
    let app = TestApp::new();
    let chat = format!(
        "<script type=\"module\" src=\"{}\"></script>",
        app.state.assets().paths().chat
    );
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (mut op, _) = operator(&app).await;
    let operators_conversation = format!("/operator/feedback/{code}");
    // Who asks: the user, or the operator.
    let pages: [(bool, &str, bool); 15] = [
        (false, "/", true),
        (false, "/plans", true),
        (false, "/download", true),
        (false, "/mac", true),
        (false, "/help", true),
        (false, "/account", true),
        (false, "/feedback", true),
        (false, "/account/password", false),
        (false, "/subscription", false),
        (false, "/no-such-page", false),
        (false, "/feedback/unread", false),
        (true, "/operator", false),
        (true, "/operator/feedback", false),
        (true, "/operator/accounts", false),
        (true, &operators_conversation, true),
    ];
    for (as_operator, path, chatting) in pages {
        let browser = if as_operator { &mut op } else { &mut user };
        let reply = browser.get(&app, path).await;
        let policy = reply.headers[CONTENT_SECURITY_POLICY].to_str().unwrap();
        if chatting {
            assert_eq!(reply.status, StatusCode::OK, "{path}");
            assert_eq!(policy, chat_policy(), "{path}");
            // The chat, and on the home page its scene.
            assert_eq!(
                reply.body.matches("<script").count(),
                1 + usize::from(path == "/"),
                "{path}"
            );
            assert!(reply.body.contains(&chat), "{path}");
        } else {
            assert_ne!(policy, chat_policy(), "{path}");
            assert!(!reply.body.contains(&chat), "{path}");
        }
    }
    // A page sent back with its form runs the chat as it did.
    let refused = user.post(&app, "/feedback", &[("body", " ")]).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.headers[CONTENT_SECURITY_POLICY], chat_policy());
    // What the script asks for runs nothing.
    let answer = user.ask(&app, "/feedback/messages?after=0").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.headers[CONTENT_SECURITY_POLICY], STRICT_POLICY);
    // The script itself is served as one, under its content's name.
    let script = user.get(&app, &app.state.assets().paths().chat).await;
    assert_eq!(script.status, StatusCode::OK);
    assert_eq!(
        script.headers[CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    assert!(script.body.contains("export function start(win)"));
}

#[tokio::test]
async fn the_link_and_order_pages_run_no_script() {
    // The page that shows a new subscription link, as its owner gets it.
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let link = user.post(&app, "/subscription/link", &[]).await;
    assert_eq!(link.status, StatusCode::OK, "{}", link.body);
    assert!(link.body.contains(&format!("value=\"{ORIGIN_URL}/sub/")));
    // An order's page, as its owner gets it.
    let shop = TestApp::shop();
    let (mut customer, _) = shop.customer("b@qq.com", "pilotuserb").await;
    let id = customer.order(&shop, "month-100").await;
    let order = customer.order_page(&shop, &id).await;
    assert_eq!(order.status, StatusCode::OK, "{}", order.body);
    // The box is there as a plain form, and nothing runs it.
    for (page, name) in [(&link, "link"), (&order, "order")] {
        assert_eq!(
            page.headers[CONTENT_SECURITY_POLICY], STRICT_POLICY,
            "{name}"
        );
        assert!(!page.body.contains("<script"), "{name}");
    }
}
