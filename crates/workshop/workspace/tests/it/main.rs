//! Integration tests for `workshop-workspace`: the registration
//! contract - routes and the workspace's state handle served through the
//! registry's contribution collections - and the save timeout on the
//! workspace routes.

mod save_timeout;

use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use tower::ServiceExt as _;
use workshop_registry::Registry;
use workshop_workspace::{Workspace, register};

#[tokio::test]
async fn the_registered_routes_serve_the_workspace_api() {
    let registry = Registry::new();
    let workspace = Workspace::new();
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("notes.txt"), "hello").expect("seed a file");
    let _guards = register(&registry, &workspace);

    let registrars = registry.routes();
    assert_eq!(registrars.len(), 1, "the routes collection is registered");
    let router = registrars[0].routes();

    // The roots listing answers through the registered routes.
    let request = Request::builder()
        .uri("/workspace/tree")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);

    // A grant over HTTP is in the roots listing of the next request.
    let body = serde_json::json!({ "path": dir.path() }).to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/workspace/grant")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("static request parts are valid");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    let request = Request::builder()
        .uri("/workspace/tree")
        .body(Body::empty())
        .expect("static request parts are valid");
    let response = router
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body is in memory already");
    let listing: serde_json::Value = serde_json::from_slice(&bytes).expect("the body is JSON");
    let roots = listing["entries"]
        .as_array()
        .expect("the listing holds its entries");
    assert_eq!(roots.len(), 1, "the grant reached the shared state");
    let granted = roots[0]["path"].as_str().expect("an entry names its path");
    let name = dir
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a named tempdir");
    assert!(
        granted.ends_with(name),
        "the granted root is the dropped directory: {granted}"
    );
}

/// Posts a JSON body naming `path` to `uri` through `router`.
#[expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]
async fn post_path(router: axum::Router, uri: &str, path: &Path) -> Response {
    let body = serde_json::json!({ "path": path }).to_string();
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("static request parts are valid");
    router
        .oneshot(request)
        .await
        .expect("the router is infallible")
}

#[tokio::test]
async fn save_workspace_as_creates_the_file_and_switches_to_it() {
    let registry = Registry::new();
    let workspace = Workspace::new();
    let home = tempfile::TempDir::new().expect("tempdir");
    let file = home.path().join("Name.rvwork");
    let _guards = register(&registry, &workspace);
    let router = registry.routes()[0].routes();

    let response = post_path(router, "/workspace/file/save-as", &file).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body is in memory already");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("the body is JSON");
    assert_eq!(json["name"], "Name", "save as switches to the new file");
    workspace.close_backing().await;
    assert!(file.is_file(), "save as creates the file");
}

#[tokio::test]
async fn dropping_the_guards_deregisters_every_contribution() {
    let registry = Registry::new();
    let workspace = Workspace::new();
    let guards = register(&registry, &workspace);
    assert!(!registry.routes().is_empty());
    assert!(registry.state::<Workspace>().is_some());
    drop(guards);
    assert!(registry.routes().is_empty());
    assert!(registry.state::<Workspace>().is_none());
}
