//! What the operator pages delete is erased from the database's files by
//! the action that deleted it, and an erasure a reader holds up is
//! reported with how to finish it.

use std::sync::mpsc;
use std::thread;

use axum::http::StatusCode;

use super::operating::binding;
use super::operator::operator;
use super::support::{DatabaseFile, TestApp};

/// Whether the database `file` or its write-ahead log holds `trace`.
fn files_hold(file: &DatabaseFile, trace: &str) -> bool {
    let log = file.path().with_file_name("portal.db-wal");
    [file.path().to_owned(), log]
        .iter()
        .any(|path| match std::fs::read(path) {
            Ok(bytes) => bytes
                .windows(trace.len())
                .any(|window| window == trace.as_bytes()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => panic!("cannot read {}: {error}", path.display()),
        })
}

/// What an action's page says when a reader held up its erasure.
const BUSY: &str = "<p class=\"caution\" role=\"alert\">操作已完成，但数据库正忙，删除的内容可能还留在数据库文件里。数据库空闲时，请在主机上运行 erase-deleted。</p>";

/// Starts a read of the database `file` on another connection, as a
/// command on the host may, and holds it open until told to end.
fn hold_a_read(file: &DatabaseFile) -> (thread::JoinHandle<()>, mpsc::Sender<()>) {
    let path = file.path().to_owned();
    let (began, begun) = mpsc::channel();
    let (end, ended) = mpsc::channel::<()>();
    let reader = thread::spawn(move || {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        let tx = conn.transaction().unwrap();
        let _: i64 = tx
            .query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))
            .unwrap();
        began.send(()).unwrap();
        ended.recv().unwrap();
        tx.commit().unwrap();
    });
    begun.recv().unwrap();
    (reader, end)
}

#[tokio::test]
async fn an_erasure_a_reader_holds_up_is_reported_with_how_to_finish_it() {
    let (app, file) = TestApp::on_disk();
    let (mut op, _) = operator(&app).await;
    let (_, alice) = app.signed_up("alice@qq.com").await;
    app.bind(alice, "pilotusera").await;
    let (_, bob) = app.signed_up("bob@qq.com").await;
    let (mut booker, _) = app.signed_up("booker@qq.com").await;
    booker.get(&app, "/mac").await;
    booker
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "month")],
        )
        .await;
    let booking = app.store(|store| store.bookings()).await.unwrap()[0]
        .booking
        .id
        .to_string();
    let (unbound, banned) = (alice.to_string(), bob.to_string());
    for (path, field, value, notice) in [
        (
            "/operator/accounts/unbind",
            "account",
            &unbound,
            format!("已解绑账号 {unbound}"),
        ),
        (
            "/operator/accounts/ban",
            "account",
            &banned,
            format!("已封禁账号 {banned}"),
        ),
        (
            "/operator/bookings/close",
            "id",
            &booking,
            format!("已关闭预订 {booking}"),
        ),
    ] {
        let (reader, end) = hold_a_read(&file);
        let done = op.post(&app, path, &[(field, value.as_str())]).await;
        end.send(()).unwrap();
        reader.join().unwrap();
        assert_eq!(done.status, StatusCode::OK, "{path}");
        assert!(done.body.contains(&notice), "{path}: {}", done.body);
        assert!(done.body.contains(BUSY), "{path}: {}", done.body);
    }
    // Each was done all the same.
    assert_eq!(binding(&app, alice).await, Some(None));
    assert_eq!(binding(&app, bob).await, None);
    assert!(
        app.store(|store| store.bookings())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn each_action_erases_what_it_deleted_from_the_files() {
    let (app, file) = TestApp::on_disk();
    let (mut op, _) = operator(&app).await;
    let (mut banned, banned_code) = app.signed_up("banned.person@qq.com").await;
    let (mut booker, _) = app.signed_up("booker@qq.com").await;
    let (_, unbound) = app.signed_up("unbound@qq.com").await;
    app.bind(unbound, "pilotuserq").await;
    banned.get(&app, "/feedback").await;
    banned
        .post(&app, "/feedback", &[("body", "question-from-the-banned")])
        .await;
    booker.get(&app, "/mac").await;
    booker
        .post(
            &app,
            "/mac/bookings",
            &[("machine", "m6-16-256"), ("period", "month")],
        )
        .await;
    let id = app.store(|store| store.bookings()).await.unwrap()[0]
        .booking
        .id
        .to_string();
    let banned_code = banned_code.to_string();
    // The booking's machine is in its row alone.
    let steps = [
        (
            "/operator/accounts/ban",
            "account",
            banned_code.clone(),
            vec![
                "banned.person@qq.com",
                banned_code.as_str(),
                "question-from-the-banned",
            ],
        ),
        (
            "/operator/accounts/unbind",
            "account",
            unbound.to_string(),
            vec!["pilotuserq"],
        ),
        ("/operator/bookings/close", "id", id, vec!["m6-16-256"]),
    ];
    for (_, _, _, traces) in &steps {
        for trace in traces {
            assert!(files_hold(&file, trace), "{trace}");
        }
    }
    // Each is gone before the next action, which erases as well, runs.
    for (path, field, value, traces) in &steps {
        let done = op.post(&app, path, &[(*field, value.as_str())]).await;
        assert_eq!(done.status, StatusCode::OK, "{path}");
        assert!(!done.body.contains("class=\"caution\""), "{path}");
        for trace in traces {
            assert!(!files_hold(&file, trace), "{path} left {trace}");
        }
    }
    // The search finds what is still there.
    assert!(files_hold(&file, "booker@qq.com"));
    assert!(
        app.store(|store| store.bookings())
            .await
            .unwrap()
            .is_empty()
    );
}
