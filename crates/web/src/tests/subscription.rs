//! The subscription page in each of its states.

use axum::http::StatusCode;

use super::support::{TestApp, snapshot_json};

#[tokio::test]
async fn an_unbound_account_has_no_subscription() {
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    let page = browser.get(&app, "/subscription").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("暂无订阅"));
    assert!(!page.body.contains("本期用量"));
}

#[tokio::test]
async fn a_bound_account_waits_for_its_first_import() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    let page = browser.get(&app, "/subscription").await;
    assert!(page.body.contains("等待数据"));
    assert!(page.body.contains("2026-11-05 绑定"));
    assert!(
        !page.body.contains("暂无订阅"),
        "bound is not 'no subscription'"
    );
    let account = browser.get(&app, "/account").await;
    assert!(account.body.contains("已绑定") && account.body.contains("绑定于 2026-11-05"));
}

#[tokio::test]
async fn an_imported_record_is_shown_with_its_figures() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import(snapshot_json("pilotusera", Some(35_000_000_000)))
        .await;
    let page = browser.get(&app, "/subscription").await.body;
    for expected in [
        "使用中",
        "35 GB",
        "40 GB",
        "接近配额 · 已用 87%",
        "width=\"87\"",
        "2026-11-01 08:00 – 2026-12-01 08:00",
        "下次重置：2026-12-01 08:00",
        "数据截至 2026-11-05 20:00",
        "不到期",
        "-reality",
    ] {
        assert!(page.contains(expected), "{expected}: {page}");
    }
}

#[tokio::test]
async fn unknown_usage_reads_unavailable_never_zero() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import(snapshot_json("pilotusera", None)).await;
    let page = browser.get(&app, "/subscription").await.body;
    assert!(page.contains("无法获取"));
    assert!(page.contains("用量无法获取，无法判断"));
    assert!(!page.contains("0 B") && !page.contains("已用 0%"));
    assert!(
        !page.contains("class=\"gauge"),
        "no bar for an unknown figure"
    );
}

#[tokio::test]
async fn a_stale_snapshot_is_flagged() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import(snapshot_json("pilotusera", Some(1))).await;
    app.clock.advance(48 * 3600 + 1).unwrap();
    let page = browser.get(&app, "/subscription").await.body;
    assert!(page.contains("超过 48 小时没有更新"));
}

#[tokio::test]
async fn a_record_of_an_ended_cycle_speaks_of_that_cycle() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import(snapshot_json("pilotusera", Some(35_000_000_000)))
        .await;
    // 2026-12-01T00:00:00Z: November's cycle is over.
    app.clock.advance(25 * 86_400 + 12 * 3600).unwrap();
    let page = browser.get(&app, "/subscription").await.body;
    for expected in [
        "这些数据属于一个已经结束的计费周期",
        "超过 48 小时没有更新",
        "该周期用量",
        "35 GB",
        "已结束的计费周期",
        "已于 2026-12-01 08:00 结束",
    ] {
        assert!(page.contains(expected), "{expected}: {page}");
    }
    for absent in ["本期用量", "下次重置", "接近配额", "class=\"gauge"] {
        assert!(!page.contains(absent), "{absent}: {page}");
    }
}

#[tokio::test]
async fn a_record_that_cannot_be_read_is_an_error_not_an_empty_state() {
    let (app, file) = TestApp::on_disk();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import(snapshot_json("pilotusera", Some(1))).await;
    // Damage the stored record behind the portal's back.
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute("UPDATE subscription_records SET document = '{}'", [])
        .unwrap();
    drop(conn);
    let page = browser.get(&app, "/subscription").await;
    assert_eq!(page.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(page.body.contains("服务器出错了"));
    assert!(!page.body.contains("暂无订阅") && !page.body.contains("等待数据"));
}
