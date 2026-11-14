//! Tests for the gateway client: the client basics live here beside the
//! shared mock-server helper; the progress, socket, switch, and timeout
//! areas each have their own submodule.

use super::*;

mod progress;
mod socket;
mod switch;
mod timeouts;

/// Binds `app` on a free loopback port and returns its base URL.
async fn serve(app: axum::Router) -> String {
    let (addr, _handle) = workshop_support::fixtures::serve(app).await;
    format!("http://{addr}")
}

#[test]
fn trailing_slash_is_trimmed_from_base_url() {
    let client = GatewayClient::new("http://127.0.0.1:8081/", "k").expect("client builds");
    assert_eq!(client.base_url, "http://127.0.0.1:8081");
}

#[test]
fn debug_redacts_the_api_key() {
    let client = GatewayClient::new("http://127.0.0.1:8081", "secret-key")
        .expect("client")
        .with_plan_proof("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
    let rendered = format!("{client:?}");
    assert!(!rendered.contains("secret-key"), "key leaked: {rendered}");
    assert!(
        !rendered.contains("0123456789abcdef"),
        "plan proof leaked: {rendered}"
    );
    assert!(
        !rendered.contains("Some") && !rendered.contains("None"),
        "presence of the proof must not leak: {rendered}"
    );
}

#[tokio::test]
async fn the_plan_proof_is_sent_on_the_model_list_and_nowhere_else() {
    use std::sync::{Arc, Mutex};

    const PROOF: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    fn echo(route: &'static str) -> axum::routing::MethodRouter {
        axum::routing::get(move |headers: axum::http::HeaderMap| async move {
            let proof = headers
                .get("x-runvil-plan-proof")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("-");
            let count = headers.get_all("x-runvil-plan-proof").iter().count();
            format!("{route} {count} {proof}")
        })
    }
    let other: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let health_slot = Arc::clone(&other);
    let forward_slot = Arc::clone(&other);
    let app = axum::Router::new()
        .route("/v1/models", echo("models"))
        .route("/admin/profiles", echo("profiles"))
        .route("/admin/status", echo("status"))
        .route(
            "/health",
            axum::routing::get(move |headers: axum::http::HeaderMap| {
                let health_slot = Arc::clone(&health_slot);
                async move {
                    health_slot.lock().expect("capture lock").extend(
                        headers
                            .get_all("x-runvil-plan-proof")
                            .iter()
                            .filter_map(|value| value.to_str().ok().map(str::to_owned)),
                    );
                    "ok"
                }
            }),
        )
        .route(
            "/admin/config",
            axum::routing::put(move |headers: axum::http::HeaderMap| {
                let forward_slot = Arc::clone(&forward_slot);
                async move {
                    forward_slot.lock().expect("capture lock").extend(
                        headers
                            .get_all("x-runvil-plan-proof")
                            .iter()
                            .filter_map(|value| value.to_str().ok().map(str::to_owned)),
                    );
                    "forwarded"
                }
            }),
        );
    let base = serve(app).await;
    let client = GatewayClient::new(&base, "tok")
        .expect("client builds")
        .with_plan_proof(PROOF);
    let models = client
        .list_models()
        .await
        .expect("the model list completes");
    assert_eq!(
        String::from_utf8_lossy(&models.body),
        format!("models 1 {PROOF}")
    );
    assert!(
        !String::from_utf8_lossy(&models.body).contains("owner"),
        "the model list body is the gateway's, not a proof"
    );
    for (read, route) in [
        (client.list_profiles().await, "profiles"),
        (client.profile_status().await, "status"),
    ] {
        let response = read.expect("the read completes");
        assert_eq!(
            String::from_utf8_lossy(&response.body),
            format!("{route} 0 -"),
            "{route} must not carry the plan proof"
        );
    }
    assert!(client.health().await, "the health probe answers");
    let forwarded = client
        .forward(reqwest::Method::PUT, "/admin/config", None)
        .await
        .expect("the forward completes");
    assert_eq!(String::from_utf8_lossy(&forwarded.body), "forwarded");
    assert!(
        other.lock().expect("capture lock").is_empty(),
        "health and the admin forward carry no plan proof"
    );
    let plain = GatewayClient::new(&base, "tok").expect("client builds");
    let models = plain.list_models().await.expect("the model list completes");
    assert_eq!(String::from_utf8_lossy(&models.body), "models 0 -");
}

#[tokio::test]
async fn each_catalog_read_sends_the_bearer_to_its_own_route() {
    fn echo(route: &'static str) -> axum::routing::MethodRouter {
        axum::routing::get(move |headers: axum::http::HeaderMap| async move {
            let auth = headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            format!("{route} {auth}")
        })
    }
    let app = axum::Router::new()
        .route("/v1/models", echo("models"))
        .route("/admin/profiles", echo("profiles"))
        .route("/admin/status", echo("status"));
    let client = GatewayClient::new(&serve(app).await, "tok").expect("client builds in tests");

    for (read, expected) in [
        (client.list_models().await, "models Bearer tok"),
        (client.list_profiles().await, "profiles Bearer tok"),
        (client.profile_status().await, "status Bearer tok"),
    ] {
        let response = read.expect("the read completes");
        assert!(
            response.status.is_success(),
            "{expected}: {}",
            response.status
        );
        assert_eq!(String::from_utf8_lossy(&response.body), expected);
    }
}
