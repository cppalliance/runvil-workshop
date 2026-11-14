//! The gateway-config panel's server side.
//!
//! Two routes serve the workshop UI's embedded config panel. `GET
//! /gateway/origin` reports the gateway's base URL so the UI can build
//! the panel iframe's address. `/gateway/api/{*path}` is the narrow
//! proxy the panel's postMessage bridge calls: it forwards the
//! gateway's `/admin/` surface (minus the progress stream) and the two
//! cache reads to the gateway with the bearer key attached, so the key
//! never reaches any browser context. The routes mount
//! inside the API group, so the [`crate::cross_site`] guard applies and
//! a non-loopback `Host` is refused; the workshop listener itself binds
//! loopback only, which keeps the whole proxy unreachable from the LAN.

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, RawQuery, State};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};

use crate::app::AppState;
use crate::error::AppError;
use workshop_gateway::ForwardedResponse;
use workshop_support::{DEFAULT_DEADLINE, with_deadline};

/// The main-origin gateway-config API routes: origin discovery (`gateway/origin`,
/// `gateway/config-origin`) and the narrow `/gateway/api/{*path}` proxy.
/// Config documents themselves live exclusively on [`config_router`] (a
/// separate loopback origin) and are never served from the main origin.
pub(crate) fn routes(state: AppState) -> Router {
    with_deadline(
        Router::new()
            .route("/gateway/origin", get(gateway_origin))
            .route("/gateway/config-origin", get(gateway_config_origin)),
        DEFAULT_DEADLINE,
    )
    .route("/gateway/api/{*path}", any(gateway_forward))
    .with_state(state)
}

/// The dedicated config-origin router: serves only the proxied Gateway
/// config SPA index (`GET /`) and its sub-assets (`GET /{*path}`).
pub(crate) fn config_router(state: AppState, parent_origin: &str) -> Router {
    let policy = crate::csp::config_policy(parent_origin);
    Router::new()
        .route("/", get(gateway_config_index))
        .route("/{*path}", get(gateway_config_assets))
        .with_state(state)
        .layer(axum::middleware::from_fn(crate::cross_site::config_guard))
        .layer(axum::middleware::from_fn(move |request, next| {
            crate::csp::stamp_config_policy(policy.clone(), request, next)
        }))
}

/// Whether the proxy forwards `method` and `path`. The rule: any method
/// on a path under `/admin/` forwards, with two exceptions: every
/// `/admin/chatgpt/` path is refused, and `GET /admin/progress` is
/// refused because the workshop owns progress display, so the panel
/// must never subscribe. Plus two admitted routes
/// outside `/admin/`: `GET /v1/cache` and `DELETE /v1/cache/<digest>`
/// for a well-formed 64-hex digest. Everything else is refused: chat
/// completions, the model list, `/health`, `/shutdown`, the config UI
/// assets, and `POST /v1/cache`. Dot segments are refused outright,
/// because the forwarding URL parse would normalize them and
/// a `..` under `/admin/` could otherwise escape onto a refused path;
/// backslashes are refused for the same reason (the WHATWG parse folds
/// them into slashes). A forwarded `/admin/` path the gateway does not
/// serve is the gateway's 404 or 405 to answer.
fn forward_allowed(method: &Method, path: &str) -> bool {
    if has_normalizing_segment(path) {
        return false;
    }
    if path == "/admin/chatgpt" || path.starts_with("/admin/chatgpt/") {
        return false;
    }
    if path.starts_with("/admin/") {
        return !(*method == Method::GET && path == "/admin/progress");
    }
    match *method {
        Method::GET => path == "/v1/cache",
        Method::DELETE => path.strip_prefix("/v1/cache/").is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        }),
        _ => false,
    }
}

/// Whether `path` holds a `.` or `..` segment or a backslash: parts the
/// forwarding URL parse would normalize, letting a path escape the prefix
/// it was admitted under.
fn has_normalizing_segment(path: &str) -> bool {
    path.split('/')
        .any(|segment| segment == "." || segment == ".." || segment.contains('\\'))
}

/// Relays a forwarded gateway response - status, content type, and body
/// byte-for-byte - adding `cache_control` when given.
fn relay(forwarded: ForwardedResponse, cache_control: Option<&'static str>) -> Response {
    let status = StatusCode::from_u16(forwarded.status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = &forwarded.content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(cache_control) = cache_control {
        builder = builder.header(header::CACHE_CONTROL, cache_control);
    }
    // The parts are valid by construction (the status came off the wire,
    // the content type round-tripped a valid header), so the build
    // cannot fail; a failure would be a bug, answered as a plain 502.
    builder
        .body(Body::from(forwarded.body))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// Answers the gateway's base URL.
async fn gateway_origin(State(state): State<AppState>) -> Response {
    let gateway = state.gateway_snapshot();
    let body = serde_json::json!({ "origin": gateway.base_url() });
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

/// Answers the dedicated config listener's loopback origin so the docked
/// config panel and its parent postMessage bridge can pin it.
async fn gateway_config_origin(State(state): State<AppState>) -> Response {
    let Some(origin) = state.config_origin() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let body = serde_json::json!({ "origin": origin });
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

/// Proxies the config SPA's index page from the gateway so the panel
/// iframe loads same-origin.
async fn gateway_config_index(State(state): State<AppState>) -> Result<Response, AppError> {
    proxy_config_asset(&state, "/config/").await
}

/// Proxies the config SPA's sub-assets (JS, CSS, icons) from the gateway.
async fn gateway_config_assets(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Response, AppError> {
    let path = format!("/config/{path}");
    if has_normalizing_segment(&path) {
        return Err(AppError::ForwardDenied);
    }
    proxy_config_asset(&state, &path).await
}

/// The shared proxy core: GET the gateway's config asset and relay it.
async fn proxy_config_asset(state: &AppState, path: &str) -> Result<Response, AppError> {
    let gateway = state.gateway_snapshot();
    let forwarded = gateway
        .client()
        .forward(reqwest::Method::GET, path, None)
        .await
        .map_err(AppError::Gateway)?;
    // The relayed SPA assets are unversioned; force revalidation so the
    // panel never runs a cached script against a newer gateway.
    Ok(relay(forwarded, Some("no-cache")))
}

/// Forwards one admitted request to the gateway with the bearer key
/// attached, relaying status, content type, and body byte-for-byte.
async fn gateway_forward(
    State(state): State<AppState>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
    method: Method,
    body: Bytes,
) -> Result<Response, AppError> {
    let path = format!("/{path}");
    if !forward_allowed(&method, &path) {
        return Err(AppError::ForwardDenied);
    }
    let path_and_query = match query {
        Some(query) => format!("{path}?{query}"),
        None => path,
    };
    // The axum and reqwest method types come from potentially different
    // `http` crate versions, so the conversion goes through the name; a
    // name reqwest cannot represent is refused rather than forwarded.
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|_| AppError::ForwardDenied)?;
    let gateway = state.gateway_snapshot();
    let forwarded = gateway
        .client()
        .forward(
            method,
            &path_and_query,
            (!body.is_empty()).then(|| body.to_vec()),
        )
        .await
        .map_err(AppError::Gateway)?;
    Ok(relay(forwarded, None))
}

#[cfg(test)]
#[path = "gateway_config-tests.rs"]
mod tests;
