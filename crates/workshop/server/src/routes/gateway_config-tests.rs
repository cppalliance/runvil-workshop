//! Gateway config proxy tests: the allowlist rule, forwarding with the bearer key, and refusals.

use super::*;

use axum::body::Body;
use axum::http::Request;
use axum::response::IntoResponse;
use axum::routing::{get as axum_get, put as axum_put};
use tower::ServiceExt;

use crate::app::router;
use crate::app::test_helpers::{body_bytes, spawn_gateway, state_for};

#[path = "gateway_config-tests-recovery.rs"]
mod recovery;

#[test]
fn the_rule_admits_the_admin_surface_and_the_cache_reads_and_refuses_the_rest() {
    for (method, path) in [
        // Every current /admin/ route the gateway serves.
        (Method::GET, "/admin/config"),
        (Method::PUT, "/admin/config"),
        (Method::GET, "/admin/config-dirty"),
        (Method::GET, "/admin/config-pending"),
        (Method::POST, "/admin/config-apply"),
        (Method::POST, "/admin/config-revert"),
        (Method::GET, "/admin/env"),
        (Method::PUT, "/admin/env"),
        (Method::GET, "/admin/chat-templates"),
        (Method::GET, "/admin/cloud-models"),
        (Method::POST, "/admin/cloud-models/refresh"),
        (Method::GET, "/admin/model-info"),
        (Method::GET, "/admin/orphans"),
        (Method::POST, "/admin/reveal"),
        (Method::GET, "/admin/status"),
        (Method::GET, "/admin/system"),
        (Method::GET, "/admin/hf/search"),
        (Method::POST, "/admin/queue/cancel"),
        (Method::POST, "/admin/queue/cancel-pending"),
        (Method::POST, "/admin/switch-profile"),
        (Method::GET, "/admin/profiles"),
        // The two cache reads outside /admin/.
        (Method::GET, "/v1/cache"),
        (
            Method::DELETE,
            "/v1/cache/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ] {
        assert!(
            forward_allowed(&method, path),
            "{method} {path} must be forwardable"
        );
    }
    for (method, path) in [
        (Method::GET, "/admin/progress"),
        (Method::GET, "/admin/chatgpt/status"),
        (Method::POST, "/admin/chatgpt/sign-in"),
        (Method::PUT, "/admin/chatgpt/models/enable"),
        (Method::PATCH, "/admin/chatgpt/sign-out"),
        (Method::DELETE, "/admin/chatgpt/reset"),
        (Method::POST, "/admin/chatgpt"),
        (Method::GET, "/admin"),
        (Method::GET, "/admin/hf/../../v1/chat/completions"),
        (Method::GET, "/admin/hf/..\\..\\v1\\chat\\completions"),
        (Method::GET, "/admin/hf/./search"),
        (Method::GET, "/v1/models"),
        (Method::POST, "/v1/chat/completions"),
        (Method::POST, "/v1/cache"),
        (Method::DELETE, "/v1/cache/abc123"),
        (Method::GET, "/health"),
        (Method::POST, "/shutdown"),
        (Method::GET, "/config/"),
    ] {
        assert!(
            !forward_allowed(&method, path),
            "{method} {path} must be refused"
        );
    }
}

#[tokio::test]
async fn the_origin_route_answers_the_configured_gateway_base_url() {
    let (state, _state_dir) = state_for("http://127.0.0.1:8081");
    let request = Request::builder()
        .uri("/gateway/origin")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router(state)
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes(response).await).expect("the body is JSON");
    assert_eq!(json["origin"], "http://127.0.0.1:8081");
}

#[tokio::test]
async fn the_proxy_forwards_an_allowlisted_path_with_the_bearer_key() {
    let gateway = axum::Router::new().route(
        "/admin/status",
        axum_get(|headers: axum::http::HeaderMap| async move {
            let authorized = headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                == Some("Bearer test-key");
            (
                [(header::CONTENT_TYPE, "application/json")],
                format!(r#"{{"profile":"default","authorized":{authorized}}}"#),
            )
        }),
    );
    let base_url = spawn_gateway(gateway).await;
    let (state, _state_dir) = state_for(&base_url);
    let request = Request::builder()
        .uri("/gateway/api/admin/status")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router(state)
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
        "the gateway's content type is relayed"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes(response).await).expect("the body is JSON");
    assert_eq!(json["profile"], "default", "the body is relayed verbatim");
    assert_eq!(
        json["authorized"], true,
        "the forward sends the workshop's bearer key"
    );
}

#[tokio::test]
async fn the_proxied_config_assets_force_revalidation_on_config_origin() {
    let gateway = axum::Router::new().route(
        "/config/app.js",
        axum_get(|| async move {
            (
                [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                "// bundle",
            )
        }),
    );
    let base_url = spawn_gateway(gateway).await;
    let (state, _state_dir) = state_for(&base_url);
    let request = Request::builder()
        .uri("/app.js")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = crate::app::config_router(state, "http://127.0.0.1:7910")
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    // The relayed bundle is unversioned; without this the panel's
    // WebView2 serves a cached script against a newer gateway.
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache"),
        "the relay forces revalidation"
    );
    let csp = response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .and_then(|value| value.to_str().ok())
        .expect("config response includes CSP");
    assert!(csp.contains("frame-ancestors http://127.0.0.1:7910"));
}

#[tokio::test]
async fn the_main_origin_no_longer_serves_gateway_config_documents() {
    let gateway = axum::Router::new()
        .route("/config/", axum_get(|| async move { "<html>ok</html>" }))
        .route("/config/app.js", axum_get(|| async move { "// ok" }));
    let base_url = spawn_gateway(gateway).await;
    let (state, _state_dir) = state_for(&base_url);
    for uri in ["/gateway/config/", "/gateway/config/app.js"] {
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("static request parts are valid"),
            )
            .await
            .expect("the router is infallible");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "main origin must not serve {uri}"
        );
    }
}

#[tokio::test]
async fn the_config_origin_refuses_non_config_paths_and_mutations() {
    let gateway = axum::Router::new()
        .route(
            "/admin/chatgpt/status",
            axum_get(|| async move { "secret" }),
        )
        .route(
            "/config/admin/chatgpt/status",
            axum_get(|| async move { "nope" }),
        );
    let base_url = spawn_gateway(gateway).await;
    let (state, _state_dir) = state_for(&base_url);
    let config_app = crate::app::config_router(state, "http://127.0.0.1:7910");

    let dot = config_app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/../admin/status")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("infallible");
    assert_eq!(dot.status(), StatusCode::FORBIDDEN);

    let post = config_app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("infallible");
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);

    let rebound = config_app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "evil.example:7910")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("infallible");
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn spawned_server_binds_a_separate_config_origin() {
    let gateway =
        axum::Router::new().route("/config/", axum_get(|| async move { "<html>panel</html>" }));
    let base_url = spawn_gateway(gateway).await;
    let config_dir = tempfile::tempdir().expect("tempdir");
    let config = workshop_support::Config {
        gateway: workshop_support::GatewayConfig {
            base_url,
            api_key: "test-key".to_string(),
        },
        server: workshop_support::ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            state_dir: config_dir.path().to_path_buf(),
        },
        agents: workshop_support::AgentsConfig::default(),
    };
    let server = crate::serve::spawn_resolved(config).expect("spawn");
    assert_ne!(server.url(), server.config_origin());

    let client = reqwest::Client::new();
    let cfg_origin_resp = client
        .get(format!("{}/gateway/config-origin", server.url()))
        .send()
        .await
        .expect("config-origin request");
    assert_eq!(cfg_origin_resp.status(), StatusCode::OK);
    let json: serde_json::Value = cfg_origin_resp.json().await.expect("json");
    assert_eq!(json["origin"], server.config_origin());

    let main_doc = client
        .get(format!("{}/", server.url()))
        .send()
        .await
        .expect("main doc request");
    assert_eq!(main_doc.status(), StatusCode::OK);
    let main_csp = main_doc
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .and_then(|v| v.to_str().ok())
        .expect("main document includes CSP");
    let expected_frame_src = format!("frame-src {}", server.config_origin());
    assert!(
        main_csp.contains(&expected_frame_src),
        "main CSP must allow only the bound config origin in frame-src: {main_csp}"
    );
    assert!(
        main_csp.contains("frame-ancestors 'none'"),
        "main CSP must keep frame-ancestors 'none': {main_csp}"
    );

    let cfg_doc = client
        .get(format!("{}/?mode=panel", server.config_origin()))
        .send()
        .await
        .expect("config doc request");
    assert_eq!(cfg_doc.status(), StatusCode::OK);
    let cfg_csp = cfg_doc
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .and_then(|v| v.to_str().ok())
        .expect("config document includes CSP");
    assert!(
        cfg_csp.contains(&format!("frame-ancestors {}", server.url())),
        "config CSP must pin frame-ancestors to main origin: {cfg_csp}"
    );
    assert!(
        !cfg_csp.contains("ipc:") && !cfg_csp.contains("ipc.localhost"),
        "config CSP must contain no Tauri IPC sources: {cfg_csp}"
    );
    assert_eq!(cfg_doc.text().await.expect("body"), "<html>panel</html>");

    let old_doc = client
        .get(format!("{}/gateway/config/", server.url()))
        .send()
        .await
        .expect("old config path request");
    assert_eq!(old_doc.status(), StatusCode::NOT_FOUND);

    server.shutdown().expect("shutdown");
}

#[tokio::test]
async fn the_proxy_forwards_the_query_string_and_a_json_body() {
    let gateway = axum::Router::new().route(
        "/admin/config",
        axum_put(
            |headers: axum::http::HeaderMap, request: axum::extract::Request| async move {
                let declared_json = headers
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    == Some("application/json");
                let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .unwrap_or_default();
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    format!(
                        r#"{{"declared_json":{declared_json},"echo":{}}}"#,
                        String::from_utf8_lossy(&body)
                    ),
                )
                    .into_response()
            },
        ),
    );
    let base_url = spawn_gateway(gateway).await;
    let (state, _state_dir) = state_for(&base_url);
    let request = Request::builder()
        .method("PUT")
        .uri("/gateway/api/admin/config?source=panel")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"active_profile":"beta"}"#))
        .expect("static request parts are valid");
    let response = router(state)
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes(response).await).expect("the body is JSON");
    assert_eq!(json["declared_json"], true, "the body forwards as JSON");
    assert_eq!(
        json["echo"]["active_profile"], "beta",
        "the body forwards verbatim"
    );
}

#[tokio::test]
async fn the_proxy_refuses_a_non_allowlisted_path_without_dialing() {
    // An unroutable gateway address: a refused path must answer 403
    // before any dial, so no transport error can occur.
    let (state, _state_dir) = state_for("http://127.0.0.1:1");
    for path in [
        "/gateway/api/v1/chat/completions",
        "/gateway/api/admin/progress",
        "/gateway/api/admin/hf/../../v1/chat/completions",
    ] {
        let request = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("static request parts are valid");
        let response = router(state.clone())
            .oneshot(request)
            .await
            .expect("the router is infallible");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "for {path}");
        let json: serde_json::Value =
            serde_json::from_slice(&body_bytes(response).await).expect("the body is JSON");
        assert_eq!(json["error"]["code"], "forward_denied", "for {path}");
    }
}

#[tokio::test]
async fn the_proxy_sits_behind_the_cross_site_guard() {
    // The workshop listener binds loopback only; on top of that the
    // cross-site guard refuses a DNS-rebound Host, so the proxy is
    // covered by the same wall as the rest of the API surface.
    let (state, _state_dir) = state_for("http://127.0.0.1:1");
    let request = Request::builder()
        .uri("/gateway/api/admin/status")
        .header("host", "rebound.example:7910")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router(state)
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes(response).await).expect("the body is JSON");
    assert_eq!(json["error"]["code"], "cross_site");
}
