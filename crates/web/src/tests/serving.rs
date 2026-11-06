//! Serving over TCP: shutting down waits for the requests in flight.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use portal_core::auth::SecretToken;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::support::{ORIGIN_URL, TestApp};

/// A login for an unknown email, as a browser on this site sends it.
fn login_request() -> String {
    let token = SecretToken::generate().unwrap().encode();
    let body = format!("csrf={token}&email=nobody%40qq.com&password=wrong+password+here");
    format!(
        "POST /login HTTP/1.1\r\nHost: portal.example\r\nOrigin: {ORIGIN_URL}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nCookie: __Host-csrf={token}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn a_request_in_flight_is_answered_before_the_server_stops() {
    let app = TestApp::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let server = tokio::spawn(crate::serve(listener, app.state.clone(), async move {
        stopped.await.unwrap();
    }));
    // With every place to hash taken, the login waits inside its handler.
    let occupied = app.state.hashing().occupy().await;
    let free = app.state.hashing().free_tickets();
    let client = tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(login_request().as_bytes()).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while app.state.hashing().free_tickets() == free {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !server.is_finished(),
        "the server must not stop with a request in flight"
    );
    drop(occupied);
    let reply = client.await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
    assert!(reply.contains("邮箱或密码不正确。"), "{reply}");
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
