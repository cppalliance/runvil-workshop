use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use crate::app::fixtures::{body_bytes, state_for};
use crate::app::router;

/// Asserts a static UI route answers 200 with the expected content type
/// and a non-empty body.
async fn assert_ui_asset(uri: &str, expected_content_type: &str) {
    let (state, _state_dir) = state_for("http://127.0.0.1:1");
    let request = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router(state)
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK, "{uri} serves");
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap_or_else(|| panic!("{uri} sets content-type"));
    assert_eq!(content_type, expected_content_type, "{uri} content type");
    assert!(
        !body_bytes(response).await.is_empty(),
        "{uri} body is non-empty"
    );
}

/// Every asset must force revalidation: the bundle is unversioned, so
/// a heuristic cache with no validator serves a stale script against a
/// newer server.
#[tokio::test]
async fn every_asset_forces_revalidation() {
    for uri in [
        "/",
        "/app.js",
        "/style.css",
        "/app.css",
        "/pcm-worklet.js",
        "/icons/promptforge-icon.png",
        "/icons/promptforge-icon@2x.png",
    ] {
        let (state, _state_dir) = state_for("http://127.0.0.1:1");
        let request = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("static request parts are valid");
        let response = router(state).oneshot(request).await.expect("infallible");
        let cache_control = response
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap_or_else(|| panic!("{uri} sets cache-control"));
        assert_eq!(cache_control, "no-cache", "{uri} cache-control");
    }
}

#[tokio::test]
async fn index_is_served_at_the_root() {
    assert_ui_asset("/", "text/html; charset=utf-8").await;
}

#[tokio::test]
async fn app_js_is_served_as_javascript() {
    assert_ui_asset("/app.js", "text/javascript; charset=utf-8").await;
}

#[tokio::test]
async fn style_css_is_served_as_css() {
    assert_ui_asset("/style.css", "text/css; charset=utf-8").await;
}

#[tokio::test]
async fn bundled_app_css_is_served_as_css() {
    assert_ui_asset("/app.css", "text/css; charset=utf-8").await;
}

#[tokio::test]
async fn pcm_worklet_is_served_as_javascript() {
    assert_ui_asset("/pcm-worklet.js", "text/javascript; charset=utf-8").await;
}

#[tokio::test]
async fn program_icon_is_served_as_png() {
    assert_ui_asset("/icons/promptforge-icon.png", "image/png").await;
}

#[tokio::test]
async fn program_icon_2x_is_served_as_png() {
    assert_ui_asset("/icons/promptforge-icon@2x.png", "image/png").await;
}

/// The code-split chunks have content-hashed names, so the test discovers
/// one through the embedded asset listing rather than naming it.
fn first_chunk_name(extension: &str) -> String {
    crate::assets::UiAssets::iter()
        .map(std::borrow::Cow::into_owned)
        .find(|name| name.starts_with("chunks/") && name.ends_with(extension))
        .unwrap_or_else(|| panic!("the UI build emits a chunks/*{extension} chunk"))
}

#[tokio::test]
async fn a_code_split_chunk_is_served_as_javascript() {
    let name = first_chunk_name(".js");
    assert_ui_asset(&format!("/{name}"), "text/javascript; charset=utf-8").await;
}

#[tokio::test]
async fn a_chunk_without_a_bundle_extension_is_not_found() {
    let (state, _state_dir) = state_for("http://127.0.0.1:1");
    let request = Request::builder()
        .uri("/chunks/app.toml")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router(state).oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_chunk_name_cannot_escape_the_asset_root() {
    // The wildcard captures the remainder raw; ui_asset's own tests pin
    // traversal refusal for the names it is handed. Here the route must
    // not 500 on a hostile name.
    let (state, _state_dir) = state_for("http://127.0.0.1:1");
    let request = Request::builder()
        .uri("/chunks/..%2F..%2FCargo.toml")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router(state).oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_missing_chunk_is_not_found() {
    let (state, _state_dir) = state_for("http://127.0.0.1:1");
    let request = Request::builder()
        .uri("/chunks/chunk-DEADBEEF00.js")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router(state).oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
