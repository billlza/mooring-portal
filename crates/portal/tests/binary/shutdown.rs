//! `serve` stops on SIGTERM and SIGINT, finishing what is in flight.

use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Starts `serve` on a free port and waits until it listens.
fn start(dir: &tempfile::TempDir) -> (Child, impl Iterator<Item = String>) {
    let db = dir.path().join("portal.db");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mooring-portal"))
        .arg("--db")
        .arg(&db)
        .args([
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--origin",
            "http://localhost:8080",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap())
        .lines()
        .map(Result::unwrap);
    let first = lines.next().unwrap();
    assert!(first.contains("serving the portal"), "{first}");
    (child, lines)
}

fn signal(child: &Child, name: &str) {
    let status = Command::new("kill")
        .args([&format!("-{name}"), &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Waits up to ten seconds for `child` to exit successfully.
fn exits_cleanly(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            return;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("serve did not stop within ten seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn serve_stops_on_sigterm() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, lines) = start(&dir);
    signal(&child, "TERM");
    exits_cleanly(&mut child);
    let rest: Vec<String> = lines.collect();
    assert!(
        rest.iter()
            .any(|line| line.contains("stopping") && line.contains("SIGTERM")),
        "{rest:?}"
    );
}

#[test]
fn serve_stops_on_sigint() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, lines) = start(&dir);
    // Sent as soon as the server says it serves: the handler is in place.
    signal(&child, "INT");
    exits_cleanly(&mut child);
    let rest: Vec<String> = lines.collect();
    assert!(
        rest.iter()
            .any(|line| line.contains("stopping") && line.contains("SIGINT")),
        "{rest:?}"
    );
}
