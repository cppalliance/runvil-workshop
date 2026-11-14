//! The menu client's loopback Gateway is contacted directly. A redirect is
//! not followed, and an `HTTP_PROXY` listener receives nothing beyond the
//! canary that proves the variable is live.

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// integration-test helpers. Failing one of these by panicking names the
// invariant the helper exists to check.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]
#![expect(
    clippy::unwrap_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::io::{Read, Write};
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use workshop_gateway::GatewayClient;

const PROOF: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ROLE: &str = "LOOPBACK_DIRECT_ROLE";
const SIGNAL: &str = "LOOPBACK_DIRECT_SIGNAL";

async fn bind() -> (String, tokio::net::TcpListener) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (format!("http://{addr}"), listener)
}

fn spawn(listener: tokio::net::TcpListener, app: Router) {
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
}

fn menu(base: &str) -> GatewayClient {
    GatewayClient::new(base, "tok")
        .expect("client")
        .with_plan_proof(PROOF)
}

fn presented_proof(headers: &HeaderMap) -> bool {
    headers
        .get("x-runvil-plan-proof")
        .and_then(|value| value.to_str().ok())
        == Some(PROOF)
}

async fn proxy_listener() -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&hits);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            recorded.fetch_add(1, Ordering::SeqCst);
            drop(socket);
        }
    });
    (format!("http://{addr}"), hits)
}

async fn canary_hits(listener: tokio::net::TcpListener, hits: Arc<AtomicUsize>) -> usize {
    let (socket, _) = listener.accept().await.expect("the child marks the canary");
    let _ = socket.readable().await;
    let mut buf = [0_u8; 1];
    let _ = socket.try_read(&mut buf);
    let seen = hits.load(Ordering::SeqCst);
    let _ = socket.writable().await;
    let _ = socket.try_write(b"k");
    seen
}

fn run_child(proxy: &str, signal: &str) -> ExitStatus {
    Command::new(std::env::current_exe().expect("the test executable"))
        .args([
            "--exact",
            "the_menu_list_does_not_use_an_http_proxy",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(ROLE, "menu")
        .env(SIGNAL, signal)
        .env("HTTP_PROXY", proxy)
        .env("ALL_PROXY", proxy)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .env_remove("REQUEST_METHOD")
        .status()
        .expect("the proxy child starts")
}

async fn prove_canary(origin: &str) {
    let client = reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .build()
        .expect("canary client");
    let finished = tokio::time::timeout(
        Duration::from_secs(5),
        client.get(format!("{origin}/v1/models")).send(),
    )
    .await;
    assert!(
        finished.is_ok(),
        "the canary hung instead of finishing through HTTP_PROXY"
    );
}

fn wait_until_counted() {
    let addr = std::env::var(SIGNAL).expect("the parent publishes the canary signal");
    let mut stream = std::net::TcpStream::connect(addr).expect("canary signal");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("write timeout");
    stream.write_all(b"c").expect("canary signal");
    let mut buf = [0_u8; 1];
    stream.read_exact(&mut buf).expect("the canary was counted");
}

#[tokio::test]
async fn the_menu_list_does_not_follow_a_redirect() {
    let hits = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&hits);
    let (dest, dest_listener) = bind().await;
    spawn(
        dest_listener,
        Router::new().route(
            "/v1/models",
            get(move || {
                let recorded = Arc::clone(&recorded);
                async move {
                    recorded.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        ),
    );
    let (origin, origin_listener) = bind().await;
    let location = format!("{dest}/v1/models");
    spawn(
        origin_listener,
        Router::new().route(
            "/v1/models",
            get(move |headers: HeaderMap| {
                let location = location.clone();
                async move {
                    assert!(
                        presented_proof(&headers),
                        "the direct request arrived without the plan proof"
                    );
                    (StatusCode::FOUND, [("location", location)])
                }
            }),
        ),
    );
    let response = menu(&origin)
        .list_models()
        .await
        .expect("the redirect is the answer");
    assert_eq!(response.status, StatusCode::FOUND);
    assert!(
        !response
            .body
            .windows(PROOF.len())
            .any(|window| window == PROOF.as_bytes())
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_menu_list_does_not_use_an_http_proxy() {
    if std::env::var(ROLE).ok().as_deref() == Some("menu") {
        let (base, listener) = bind().await;
        spawn(
            listener,
            Router::new().route(
                "/v1/models",
                get(move |headers: HeaderMap| async move {
                    assert!(
                        presented_proof(&headers),
                        "the direct request arrived without the plan proof"
                    );
                    "models"
                }),
            ),
        );
        prove_canary(&base).await;
        wait_until_counted();
        let response = menu(&base)
            .list_models()
            .await
            .expect("the loopback list completes directly");
        assert!(response.status.is_success());
        return;
    }
    let (proxy, hits) = proxy_listener().await;
    let gate = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gate_addr = gate.local_addr().unwrap().to_string();
    let recorded = Arc::clone(&hits);
    let marked = tokio::spawn(canary_hits(gate, recorded));
    let status = tokio::task::spawn_blocking(move || run_child(&proxy, &gate_addr))
        .await
        .expect("the child join");
    assert!(status.success(), "the proxy child exited {status}");
    let canary = marked.await.expect("the canary mark");
    assert!(
        canary >= 1,
        "HTTP_PROXY was not consulted, so the direct-client check proves nothing"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        canary,
        "the loopback client contacted the proxy"
    );
}
