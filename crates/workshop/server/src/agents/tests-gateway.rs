//! The Gateway that every tool call of a library Plugin is lent:
//! `reachable_gateway` reads the endpoint and the key of the gateway the
//! registry holds now, so a replaced gateway is the one read next, and it
//! reads none when no gateway is registered, when the heartbeat reports
//! the gateway down, and when the gateway's key cannot build.

use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use harness_gateway_client::{GatewayBroker, GatewayEndpoint, SecretString};
use runvil::harness::InferenceBroker;
use serde_json::json;
use workshop_gateway::{GatewayBinding, GatewayHandles, GatewayHealth};
use workshop_registry::{Registration, Registry};

use crate::agents::reachable_gateway;
use crate::app::test_helpers::spawn_gateway;

/// The base URL of a gateway that a test registers and never calls.
const UNCALLED: &str = "http://127.0.0.1:9";

/// A registry holding gateway handles bound to `base_url` under `key`,
/// with the binding, the health flag, and the guard that keeps the
/// registration alive.
fn registry_with_gateway(
    base_url: &str,
    key: &str,
) -> (Registry, GatewayBinding, GatewayHealth, Registration) {
    let registry = Registry::new();
    let binding = GatewayBinding::new(base_url, key).expect("the binding builds");
    let health = GatewayHealth::new();
    let guard = workshop_gateway::register(
        &registry,
        GatewayHandles::new(binding.clone(), health.clone()),
    );
    (registry, binding, health, guard)
}

/// A mock Gateway keyed with `key` that lists one model named `model`; a
/// request under another key is refused.
fn keyed_gateway(model: &'static str, key: &'static str) -> Router {
    Router::new().route(
        "/v1/models",
        get(move |headers: HeaderMap| async move {
            let keyed = headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|bearer| bearer == format!("Bearer {key}"));
            if !keyed {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            axum::Json(json!({ "data": [{
                "id": model, "description": model, "context": 131_072, "thinking": "never",
            }] }))
            .into_response()
        }),
    )
}

/// The names of the models the Gateway at `gateway`'s endpoint lists when
/// it is asked under `gateway`'s key. Only the Gateway client's own crate
/// reads a key, so a request under it is how a test tells which key it
/// is.
async fn listed(gateway: (GatewayEndpoint, SecretString)) -> Vec<String> {
    let (endpoint, key) = gateway;
    let catalog = GatewayBroker::new(endpoint, key)
        .models()
        .await
        .expect("the gateway at the endpoint admits the key");
    catalog
        .models()
        .iter()
        .map(|model| model.id().name().to_owned())
        .collect()
}

#[tokio::test]
async fn a_replaced_gateway_is_the_one_read_next() {
    let first = spawn_gateway(keyed_gateway("first-model", "first-key")).await;
    let second = spawn_gateway(keyed_gateway("second-model", "second-key")).await;
    let (registry, binding, _health, _guard) = registry_with_gateway(&first, "first-key");

    let before = reachable_gateway(&registry).expect("the registered gateway is read");
    assert_eq!(
        before.0.url(),
        format!("{first}/v1"),
        "the endpoint is the gateway's API root"
    );
    assert_eq!(listed(before).await, ["first-model"]);

    binding
        .replace(&second, "second-key")
        .expect("the replacement publishes");
    let after = reachable_gateway(&registry).expect("the replacement is read");
    assert_eq!(
        after.0.url(),
        format!("{second}/v1"),
        "the new generation's endpoint is read"
    );
    assert_eq!(
        listed(after).await,
        ["second-model"],
        "the new generation's key is read with its endpoint"
    );
}

#[test]
fn without_a_gateway_registration_no_gateway_is_read() {
    assert!(reachable_gateway(&Registry::new()).is_none());
}

#[test]
fn a_gateway_the_heartbeat_reports_down_is_not_read() {
    let (registry, _binding, health, _guard) = registry_with_gateway(UNCALLED, "test-key");
    assert!(
        reachable_gateway(&registry).is_some(),
        "a gateway the heartbeat reports up is read"
    );

    health.publish(false);
    assert!(
        reachable_gateway(&registry).is_none(),
        "a gateway reported down is left out, so no tool call is lent it"
    );
}

#[test]
fn a_gateway_whose_key_cannot_build_is_not_read() {
    let (registry, _binding, _health, _guard) = registry_with_gateway(UNCALLED, "");
    assert!(
        reachable_gateway(&registry).is_none(),
        "an empty key builds no bearer"
    );
}
