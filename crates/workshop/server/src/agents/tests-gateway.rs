//! The Gateway that every tool call of a library Plugin is lent:
//! `reachable_gateway` reads the endpoint and the key of the gateway the
//! registry holds now, so a replaced gateway is the one read next, and it
//! reads none when no gateway is registered, when the heartbeat reports
//! the gateway down, and when the gateway's key cannot build. A prompt's
//! web search, through the web library Plugin, is lent the gateway that is
//! registered when the search starts: a Host built under one gateway
//! searches the gateway that replaced it.

use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use harness_gateway_client::{GatewayBroker, GatewayEndpoint, SecretString};
use runvil::harness::InferenceBroker;
use serde_json::{Value, json};
use workshop_gateway::{GatewayBinding, GatewayHandles, GatewayHealth};
use workshop_registry::{Registration, Registry};

use super::{WEB_LIBRARY, assert_no_failures, plugin_dir_of, run_prompt, serving};
use crate::agents::{reachable_gateway, server_host};
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

/// Whether a request with `headers` carries `key` as its bearer.
fn carries(headers: &HeaderMap, key: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|bearer| bearer == format!("Bearer {key}"))
}

/// A mock Gateway keyed with `key` that lists one model named `model`; a
/// request under another key is refused.
fn keyed_gateway(model: &'static str, key: &'static str) -> Router {
    Router::new().route(
        "/v1/models",
        get(move |headers: HeaderMap| async move {
            if !carries(&headers, key) {
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

/// A mock Gateway keyed with `key` whose web search answers the query it
/// is asked with one result titled `title`; a search under another key
/// is refused.
fn searching_gateway(key: &'static str, title: &'static str) -> Router {
    Router::new().route(
        "/v1/tools/web_search",
        post(
            move |headers: HeaderMap, axum::Json(asked): axum::Json<Value>| async move {
                if !carries(&headers, key) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                axum::Json(json!({ "query": asked["query"], "results": [{
                    "title": title, "url": "https://example.com/docs", "description": "The manual.",
                }] }))
                .into_response()
            },
        ),
    )
}

/// A prompt that declares `web` and returns what `web/search` answers
/// for the query `docs`.
const SEARCHES: &str = "---\nname: searches\ndescription: searches the web\nrunvil: 0\n\
    uses:\n  - web\n---\n\n## Only\n\n```lua\n\
    return tools.call('web/search', { query = 'docs' })\n```\n";

#[tokio::test]
async fn a_prompts_web_search_asks_the_gateway_registered_when_the_search_starts() {
    let first = spawn_gateway(searching_gateway("first-key", "First")).await;
    let second = spawn_gateway(searching_gateway("second-key", "Second")).await;
    let (registry, binding, _health, _guard) = registry_with_gateway(&first, "first-key");
    let (dir, _library) = plugin_dir_of("searches-the-web", WEB_LIBRARY);
    let host = server_host(serving(SEARCHES), &registry, None, Some(dir.path()));
    assert_no_failures(&host);
    // The Host is built under the first gateway, and the second replaces
    // it before any search starts.
    binding
        .replace(&second, "second-key")
        .expect("the replacement publishes");

    let result = run_prompt(host).await;
    let Ok(Ok(answer)) = result else {
        panic!("the run ends with the search's answer: {result:?}");
    };
    // The web Pack hands the gateway's results on as compact JSON.
    let results = r#"{"query":"docs","results":[{"title":"Second","url":"https://example.com/docs","description":"The manual."}]}"#;
    assert!(
        answer.contains(results),
        "the search asks the gateway that replaced the first, under its key, \
         and its results reach the script: {answer}"
    );
    assert!(
        answer.contains("<untrusted_input_"),
        "search results reach the script marked as untrusted: {answer}"
    );
}
