//! The Content-Security-Policy stamped on every server response.
//!
//! The desktop app loads the UI as an External-origin Tauri webview, so
//! the page's policy is the server's to set: there is no `tauri.conf.json`
//! CSP for a remote document. The policy keeps the SPA self-contained -
//! scripts and workers from this origin only - while `connect-src` admits
//! the two IPC endpoints Tauri calls from an External origin (`ipc:` on
//! the custom-protocol platforms, `http://ipc.localhost` under WebView2)
//! and the loopback WebSocket spellings. WebKit does not treat
//! `connect-src 'self'` as covering WebSockets, so the `ws://` sources
//! are spelled out for WebKitGTK and WKWebView; the port wildcard covers
//! the desktop app's OS-assigned bind.

use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

/// The policy value prefix for every main-origin response (before `frame-src`).
///
/// `img-src` keeps `https:` so model-written markdown can still render
/// remote images. `style-src` keeps `'unsafe-inline'` because the UI's
/// styling is partly runtime-injected and cannot be nonced: CodeMirror 6
/// mounts its modules as `<style>` elements through style-mod, and Shiki
/// emits inline `style` color attributes, which Chromium gates through
/// the `style-src-attr` fallback to `style-src`. The directives that gate
/// code execution and connections stay strict, `frame-src` is pinned
/// exclusively to the separate config listener's exact origin once bound,
/// and `frame-ancestors 'none'` forbids framing everywhere on the main origin.
const POLICY_PREFIX: &str = "default-src 'self'; script-src 'self'; \
                             style-src 'self' 'unsafe-inline'; \
                             connect-src 'self' ipc: http://ipc.localhost ws://127.0.0.1:* \
                             ws://localhost:* ws://[::1]:*; img-src 'self' data: blob: https:; \
                             worker-src 'self'; object-src 'none'; base-uri 'none'";

const POLICY_NO_FRAMES: &str = "default-src 'self'; script-src 'self'; \
                                style-src 'self' 'unsafe-inline'; \
                                connect-src 'self' ipc: http://ipc.localhost ws://127.0.0.1:* \
                                ws://localhost:* ws://[::1]:*; img-src 'self' data: blob: https:; \
                                worker-src 'self'; object-src 'none'; base-uri 'none'; \
                                frame-src 'none'; frame-ancestors 'none'";

/// Builds the main listener's CSP, pinning `frame-src` solely to `config_origin`.
pub(crate) fn main_policy_for(config_origin: Option<&str>) -> HeaderValue {
    match config_origin {
        Some(origin) => {
            let policy = format!("{POLICY_PREFIX}; frame-src {origin}; frame-ancestors 'none'");
            HeaderValue::from_str(&policy)
                .unwrap_or_else(|_| HeaderValue::from_static(POLICY_NO_FRAMES))
        }
        None => HeaderValue::from_static(POLICY_NO_FRAMES),
    }
}

/// Builds the CSP stamped on every response from the separate config
/// listener: restricts framing exclusively to `parent_origin` (the
/// Workshop window's exact loopback origin) while keeping script/worker
/// execution scoped to the config origin itself and withholding all
/// Tauri IPC `connect-src` allowances.
pub(crate) fn config_policy(parent_origin: &str) -> HeaderValue {
    let policy = format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         connect-src 'self'; img-src 'self' data: blob:; \
         worker-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; \
         frame-ancestors {parent_origin}"
    );
    HeaderValue::from_str(&policy).unwrap_or_else(|_| HeaderValue::from_static(POLICY_NO_FRAMES))
}

/// Middleware stamping `Content-Security-Policy` on every main-listener
/// response, including error envelopes.
pub(crate) async fn header(
    axum::extract::State(state): axum::extract::State<crate::app::AppState>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, state.main_csp());
    response
}

/// Middleware stamping the config listener's CSP on every response,
/// including error envelopes.
pub(crate) async fn stamp_config_policy(
    policy: HeaderValue,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, policy);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::app::router;
    use crate::app::test_helpers::state_for;

    /// Fetches `uri` against the composed router and returns the response.
    async fn get(uri: &str) -> axum::response::Response {
        let (state, _state_dir) = state_for("http://127.0.0.1:1");
        router(state)
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("static request parts are valid"),
            )
            .await
            .expect("the router is infallible")
    }

    #[tokio::test]
    async fn every_response_is_stamped_with_the_policy() {
        // The UI document, its asset, the health probe, and an API error
        // envelope: the header is stamped at the composition root, so no
        // response class escapes it.
        for uri in ["/", "/app.js", "/health", "/workspace/tree"] {
            let response = get(uri).await;
            let value = response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap_or_else(|| panic!("{uri} is stamped with the policy"));
            assert_eq!(
                value, POLICY_NO_FRAMES,
                "{uri} is stamped with the exact policy"
            );
        }
    }

    #[tokio::test]
    async fn the_main_router_forbids_framing_everywhere() {
        for uri in ["/", "/app.js", "/health", "/gateway/config/"] {
            let response = get(uri).await;
            let value = response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap_or_else(|| panic!("{uri} is stamped with the policy"));
            assert_eq!(value, POLICY_NO_FRAMES, "{uri} forbids framing");
        }
    }

    #[tokio::test]
    async fn the_main_router_pins_frame_src_exclusively_to_config_origin() {
        let (state, _state_dir) = state_for("http://127.0.0.1:1");
        let config_origin = "http://127.0.0.1:49999";
        state.set_config_origin(config_origin.to_owned());
        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("static request parts are valid"),
            )
            .await
            .expect("the router is infallible");
        let value = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok())
            .expect("main document is stamped with CSP");
        assert!(
            value.contains("frame-src http://127.0.0.1:49999"),
            "main CSP must admit only the bound config origin in frame-src: {value}"
        );
        assert!(
            !value.contains("frame-src http://127.0.0.1:*") && !value.contains("frame-src 'self'"),
            "main CSP must not use a wildcard or self frame-src: {value}"
        );
        assert!(
            value.contains("frame-ancestors 'none'"),
            "main CSP must keep frame-ancestors 'none': {value}"
        );
    }

    #[tokio::test]
    async fn the_config_router_pins_frame_ancestors_to_main_origin() {
        let (state, _state_dir) = state_for("http://127.0.0.1:1");
        let main_origin = "http://127.0.0.1:49152";
        let response = crate::app::config_router(state, main_origin)
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("static request parts are valid"),
            )
            .await
            .expect("the router is infallible");
        let value = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok())
            .expect("the config response is stamped with CSP");
        assert!(
            value.contains("frame-ancestors http://127.0.0.1:49152"),
            "config CSP must name the exact parent origin: {value}"
        );
        assert!(
            !value.contains("frame-ancestors 'self'"),
            "config CSP must not allow arbitrary same-origin framing: {value}"
        );
        assert!(
            !value.contains("ipc:") && !value.contains("ipc.localhost"),
            "config CSP must grant no Tauri IPC sources: {value}"
        );
    }

    #[tokio::test]
    async fn the_policy_admits_tauri_ipc_and_the_loopback_socket() {
        let response = get("/health").await;
        assert_eq!(response.status(), StatusCode::OK);
        let policy = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("the policy header is present")
            .to_str()
            .expect("the policy is ASCII");
        // The break this pins: drop the IPC sources and the desktop app webview's
        // Tauri calls fail closed from the External origin.
        assert!(
            policy.contains("connect-src 'self' ipc: http://ipc.localhost"),
            "connect-src admits the Tauri IPC endpoints: {policy}"
        );
        assert!(
            policy.contains("ws://127.0.0.1:*"),
            "connect-src admits the loopback WebSocket: {policy}"
        );
    }

    #[tokio::test]
    async fn the_policy_admits_runtime_injected_styles() {
        let response = get("/health").await;
        let policy = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("the policy header is present")
            .to_str()
            .expect("the policy is ASCII");
        // The break this pins: a self-only style policy refuses the
        // `<style>` elements CodeMirror 6 mounts through style-mod and
        // the inline `style` color attributes Shiki emits (Chromium gates
        // attributes through the style-src-attr fallback), leaving the
        // editor unstyled and code blocks uncolored.
        assert!(
            policy.contains("style-src 'self' 'unsafe-inline'"),
            "style-src admits the UI's runtime-injected styles: {policy}"
        );
    }
}
