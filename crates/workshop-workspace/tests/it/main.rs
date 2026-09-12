//! Integration tests for `workshop-workspace`: the registration
//! contract - routes and the granted-roots handle served through the
//! registry's slots.

use axum::body::Body;
use axum::http::{Request, StatusCode};
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

    let registrar = registry
        .workspace_routes()
        .get()
        .expect("the routes slot is registered");
    let router = registrar.routes();

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

    // A grant over HTTP becomes visible through the registered roots
    // handle immediately.
    let body = serde_json::json!({ "path": dir.path() }).to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/workspace/grant")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("static request parts are valid");
    let response = router
        .oneshot(request)
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    let roots = registry
        .workspace_roots()
        .get()
        .expect("the roots slot is registered")
        .granted_roots();
    assert_eq!(roots.len(), 1, "the grant reached the shared state");
    assert!(
        roots[0].ends_with(dir.path().file_name().expect("a named tempdir")),
        "the granted root is the dropped directory: {roots:?}"
    );
}

#[tokio::test]
async fn dropping_the_guards_deregisters_both_slots() {
    let registry = Registry::new();
    let workspace = Workspace::new();
    let guards = register(&registry, &workspace);
    assert!(registry.workspace_routes().get().is_some());
    assert!(registry.workspace_roots().get().is_some());
    drop(guards);
    assert!(registry.workspace_routes().get().is_none());
    assert!(registry.workspace_roots().get().is_none());
}
