//! The Mac rental page: anyone may look, an account books for itself
//! alone, and nothing on the page or in its forms names an account.

use axum::http::StatusCode;
use portal_core::rental::{MACHINES, Period};

use super::support::{Browser, PASSWORD, TestApp};

/// What the store holds of every booking: account code, machine, period
/// and the price per period.
async fn held(app: &TestApp) -> Vec<(String, String, Period, u64)> {
    app.store(|store| store.bookings().unwrap())
        .await
        .into_iter()
        .map(|listed| {
            (
                listed.account.to_string(),
                listed.booking.terms.machine.to_string(),
                listed.booking.terms.period,
                listed.booking.terms.price,
            )
        })
        .collect()
}

#[tokio::test]
async fn anyone_sees_every_machine_with_its_prices() {
    let app = TestApp::new();
    let mut visitor = Browser::new([203, 0, 113, 1]);
    let page = visitor.get(&app, "/mac").await;
    assert_eq!(page.status, StatusCode::OK);
    let body = page.body;
    for machine in &MACHINES {
        assert!(
            body.contains(&format!(
                "<input type=\"radio\" name=\"machine\" value=\"{}\"",
                machine.id
            )),
            "{}",
            machine.id
        );
    }
    // The first machine and the month start chosen.
    assert!(body.contains("name=\"machine\" value=\"m4-16-256\" checked required"));
    assert!(body.contains("name=\"period\" value=\"month\" checked required"));
    assert_eq!(body.matches(" checked").count(), 2);
    // Every machine has a price for every period, which the stylesheet
    // shows by the period chosen.
    for period in Period::ALL {
        assert_eq!(
            body.matches(&format!("data-period=\"{}\"", period.as_str()))
                .count(),
            MACHINES.len(),
            "{period:?}"
        );
    }
    for price in ["£79.00", "首月 £47.40", "£2234.00", "首月省 40%", "省 15%"] {
        assert!(body.contains(price), "{price}");
    }
    assert!(body.contains("最快第二天开通"));
    // A visitor is told to sign in, and has no bookings to see.
    assert!(body.contains("预订前要先登录"));
    assert!(!body.contains("我的预订"));
    assert!(body.contains("<a href=\"/mac\" aria-current=\"page\">租 Mac</a>"));
    // In English too.
    let switched = visitor
        .post(&app, "/lang", &[("to", "en"), ("back", "mac")])
        .await;
    assert_eq!(switched.location(), "/mac");
    let english = visitor.get(&app, "/mac").await.body;
    assert!(english.contains("<h1>Rent a Mac mini</h1>"));
    assert!(english.contains("first month £47.40"));
    // Short, so that the bar still fits just above where it folds.
    assert!(english.contains("<a href=\"/mac\" aria-current=\"page\">Mac mini</a>"));
}

#[tokio::test]
async fn the_stylesheet_shows_each_periods_price_when_it_is_chosen() {
    let app = TestApp::new();
    let css = Browser::new([203, 0, 113, 1])
        .get(&app, "/assets/site.css")
        .await
        .body;
    // Every price is hidden only where the browser can show the chosen one.
    assert!(
        css.contains(
            "@supports selector(:has(*)) {\n  .rental .machine-price {\n    display: none;"
        )
    );
    for period in Period::ALL {
        let id = period.as_str();
        assert!(
            css.contains(&format!(
                ".rental:has([name=\"period\"][value=\"{id}\"]:checked) [data-period=\"{id}\"]"
            )),
            "{id}"
        );
    }
}

#[tokio::test]
async fn a_visitor_is_sent_to_log_in_and_comes_back_to_the_page() {
    let app = TestApp::new();
    let mut visitor = Browser::new([203, 0, 113, 1]);
    visitor.get(&app, "/mac").await;
    for (path, fields) in [
        (
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "month")][..],
        ),
        ("/mac/bookings/withdraw", &[("machine", "m6-16-256")][..]),
    ] {
        let reply = visitor.post(&app, path, fields).await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(reply.location(), "/login?next=mac", "{path}");
    }
    assert_eq!(held(&app).await, []);
    app.signed_up("alice@qq.com").await;
    let login = visitor.get(&app, "/login?next=mac").await.body;
    assert!(login.contains("<input type=\"hidden\" name=\"next\" value=\"mac\">"));
    let reply = visitor
        .post(
            &app,
            "/login",
            &[
                ("email", "alice@qq.com"),
                ("password", PASSWORD),
                ("next", "mac"),
            ],
        )
        .await;
    assert_eq!(reply.location(), "/mac");
}

#[tokio::test]
async fn an_account_books_a_machine_and_withdraws_it() {
    let app = TestApp::new();
    let (mut alice, code) = app.signed_up("alice@qq.com").await;
    let page = alice.get(&app, "/mac").await.body;
    assert!(!page.contains("预订前要先登录"));
    let reply = alice
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-24-512"), ("period", "quarter")],
        )
        .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location(), "/mac?notice=booked");
    assert_eq!(
        held(&app).await,
        [(
            code.to_string(),
            "m6-24-512".to_owned(),
            Period::Quarter,
            45_600
        )]
    );
    let page = alice.get(&app, reply.location()).await.body;
    assert!(page.contains("已收到你的预订"), "{page}");
    assert!(page.contains("我的预订"));
    assert!(page.contains("M6 · 24GB · 512GB · 按季"));
    assert!(page.contains("£456.00/季"));
    assert!(page.contains("预订于 2026-11-05 20:00 北京时间"));
    assert!(page.contains("<input type=\"hidden\" name=\"machine\" value=\"m6-24-512\">"));
    // Booked again for the month, the booking changes; the first month
    // costs less.
    alice
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-24-512"), ("period", "month")],
        )
        .await;
    assert_eq!(
        held(&app).await,
        [(
            code.to_string(),
            "m6-24-512".to_owned(),
            Period::Month,
            16_900
        )]
    );
    let page = alice.get(&app, "/mac").await.body;
    assert!(page.contains("£169.00/月<span class=\"block muted small\">首月 £101.40</span>"));
    let reply = alice
        .post(&app, "/mac/bookings/withdraw", &[("machine", "m6-24-512")])
        .await;
    assert_eq!(reply.location(), "/mac?notice=withdrawn");
    assert_eq!(held(&app).await, []);
    let page = alice.get(&app, reply.location()).await.body;
    assert!(page.contains("预订已撤回"));
    assert!(!page.contains("我的预订"));
    // Withdrawn twice (a second click), the answer is the same.
    let again = alice
        .post(&app, "/mac/bookings/withdraw", &[("machine", "m6-24-512")])
        .await;
    assert_eq!(again.location(), "/mac?notice=withdrawn");
}

#[tokio::test]
async fn an_account_never_sees_or_withdraws_another_accounts_booking() {
    let app = TestApp::new();
    let (mut alice, alice_code) = app.signed_up("alice@qq.com").await;
    let (mut bob, bob_code) = app.signed_up("bob@163.com").await;
    alice
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m5pro-24-512"), ("period", "year")],
        )
        .await;
    bob.post(
        &app,
        "/mac/bookings",
        &[("machine", "m4-16-256"), ("period", "day")],
    )
    .await;
    let bobs = bob.get(&app, "/mac").await.body;
    assert!(bobs.contains("M4 · 16GB · 256GB · 按天"));
    assert!(!bobs.contains("M5 Pro · 24GB"), "{bobs}");
    // Bob withdrawing the machine Alice booked takes nothing of hers, and
    // nothing he adds to the form names her.
    let reply = bob
        .post(
            &app,
            "/mac/bookings/withdraw",
            &[
                ("machine", "m5pro-24-512"),
                ("account", &alice_code.to_string()),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(
        held(&app).await,
        [
            (
                alice_code.to_string(),
                "m5pro-24-512".to_owned(),
                Period::Year,
                223_400
            ),
            (
                bob_code.to_string(),
                "m4-16-256".to_owned(),
                Period::Day,
                1_000
            ),
        ]
    );
    let alices = alice.get(&app, "/mac").await.body;
    assert!(alices.contains("M5 Pro · 24GB · 512GB · 按年"));
    assert!(!alices.contains("M4 · 16GB · 256GB"));
}

#[tokio::test]
async fn a_booking_takes_its_terms_from_the_offer_not_the_form() {
    let app = TestApp::new();
    let (mut alice, _) = app.signed_up("alice@qq.com").await;
    // Prices and specifications in the form are not read.
    alice
        .post(
            &app,
            "/mac/bookings",
            &[
                ("machine", "m6-16-256"),
                ("period", "month"),
                ("price", "1"),
                ("chip", "M9"),
            ],
        )
        .await;
    let booked = app.store(|store| store.bookings().unwrap()).await;
    assert_eq!(booked.len(), 1);
    let terms = &booked[0].booking.terms;
    assert_eq!(
        (
            terms.chip.as_str(),
            terms.memory_gb,
            terms.price,
            terms.first_price
        ),
        ("M6", 16, 11_900, 7_140)
    );
}

#[tokio::test]
async fn a_machine_off_the_offer_or_a_period_not_on_it_is_not_booked() {
    let app = TestApp::new();
    let (mut alice, _) = app.signed_up("alice@qq.com").await;
    let gone = alice
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m3-8-256"), ("period", "month")],
        )
        .await;
    assert_eq!(gone.location(), "/mac?notice=machine_gone");
    let page = alice.get(&app, gone.location()).await.body;
    assert!(page.contains("这个配置已经不再提供"));
    for (path, fields) in [
        (
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "hour")][..],
        ),
        ("/mac/bookings", &[("machine", "m6-16-256")][..]),
        ("/mac/bookings/withdraw", &[("machine", "M6 Pro")][..]),
        ("/mac/bookings/withdraw", &[][..]),
    ] {
        let reply = alice.post(&app, path, fields).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{path} {fields:?}");
    }
    assert_eq!(held(&app).await, []);
    // A notice that is not one is the site's own error, not the page.
    let odd = alice.get(&app, "/mac?notice=paid").await;
    assert_eq!(odd.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_session_that_ends_before_its_form_arrives_books_nothing() {
    let app = TestApp::new();
    let (mut alice, code) = app.signed_up("alice@qq.com").await;
    alice.get(&app, "/mac").await;
    // The operator deletes the account while the form is on its way.
    app.store(move |store| store.delete_account(&code).unwrap())
        .await;
    let reply = alice
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "month")],
        )
        .await;
    assert_eq!(reply.location(), "/login?next=mac");
    let reply = alice
        .post(&app, "/mac/bookings/withdraw", &[("machine", "m6-16-256")])
        .await;
    assert_eq!(reply.location(), "/login?next=mac");
    assert_eq!(held(&app).await, []);
}

#[tokio::test]
async fn bookings_that_cannot_be_read_are_an_error_never_an_empty_list() {
    let (app, file) = TestApp::on_disk();
    let (mut alice, _) = app.signed_up("alice@qq.com").await;
    alice
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "month")],
        )
        .await;
    rusqlite::Connection::open(file.path())
        .unwrap()
        .execute_batch("DROP TABLE mac_bookings")
        .unwrap();
    let page = alice.get(&app, "/mac").await;
    assert_eq!(page.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(page.body.contains("服务器出错了"));
    assert!(!page.body.contains("选择配置"));
    for (path, fields) in [
        (
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "week")][..],
        ),
        ("/mac/bookings/withdraw", &[("machine", "m6-16-256")][..]),
    ] {
        let reply = alice.post(&app, path, fields).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
    }
    // A visitor's page reads no bookings, so it is still the offer.
    let mut visitor = Browser::new([203, 0, 113, 2]);
    let offer = visitor.get(&app, "/mac").await;
    assert_eq!(offer.status, StatusCode::OK);
    assert!(offer.body.contains("选择配置"));
}
