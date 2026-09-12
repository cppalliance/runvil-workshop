//! Switch-profile tests: the wire shapes decode, an accepted switch
//! streams stages then its terminal event, malformed payloads are
//! skipped, and a declined switch is buffered rather than an error.

use super::*;

use futures_util::StreamExt as _;

#[test]
fn switch_event_decodes_each_wire_shape() {
    let stage: SwitchEvent =
        serde_json::from_str(r#"{"stage":"stopping-models"}"#).expect("a stage marker decodes");
    assert_eq!(
        stage,
        SwitchEvent::Stage {
            stage: "stopping-models".to_string()
        }
    );
    let ready: SwitchEvent =
        serde_json::from_str(r#"{"status":"ready","profile":"beta"}"#).expect("ready decodes");
    assert_eq!(
        ready,
        SwitchEvent::Ready {
            profile: "beta".to_string()
        }
    );
    let error: SwitchEvent =
        serde_json::from_str(r#"{"status":"error","message":"boom"}"#).expect("error decodes");
    assert_eq!(
        error,
        SwitchEvent::Error {
            message: "boom".to_string()
        }
    );
}

/// Collects the typed events of a switch stream, panicking on a
/// transport error item.
async fn collect_switch_events(payloads: SsePayloadStream) -> Vec<SwitchEvent> {
    let mut events = Vec::new();
    let mut typed = switch_events(payloads);
    while let Some(item) = typed.next().await {
        events.push(item.expect("the stream is clean"));
    }
    events
}

#[tokio::test]
async fn an_accepted_switch_streams_stages_then_the_terminal_event() {
    let app = axum::Router::new().route(
        "/admin/switch-profile",
        axum::routing::post(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                concat!(
                    "data: {\"stage\":\"loading-profile\"}\n\n",
                    "data: {\"stage\":\"stopping-models\"}\n\n",
                    "data: {\"stage\":\"starting-models\"}\n\n",
                    "data: {\"status\":\"ready\",\"profile\":\"beta\"}\n\n",
                ),
            )
        }),
    );
    let base_url = serve(app).await;
    let client = GatewayClient::new(&base_url, "").expect("client builds in tests");
    let response = client
        .switch_profile("beta")
        .await
        .expect("the request completes");
    let SwitchResponse::Switching { payloads, .. } = response else {
        panic!("an accepted switch streams, got {response:?}");
    };
    assert_eq!(
        collect_switch_events(payloads).await,
        [
            SwitchEvent::Stage {
                stage: "loading-profile".to_string()
            },
            SwitchEvent::Stage {
                stage: "stopping-models".to_string()
            },
            SwitchEvent::Stage {
                stage: "starting-models".to_string()
            },
            SwitchEvent::Ready {
                profile: "beta".to_string()
            },
        ],
        "the stream carries stage markers in order then the terminal ready"
    );
}

#[tokio::test]
async fn a_malformed_switch_event_is_skipped_and_the_stream_continues() {
    let app = axum::Router::new().route(
        "/admin/switch-profile",
        axum::routing::post(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                concat!(
                    "data: {\"stage\":\"loading-profile\"}\n\n",
                    "data: this is not json\n\n",
                    "data: {\"unrelated\":true}\n\n",
                    "data: {\"status\":\"error\",\"message\":\"start-local failed\"}\n\n",
                ),
            )
        }),
    );
    let base_url = serve(app).await;
    let client = GatewayClient::new(&base_url, "").expect("client builds in tests");
    let response = client
        .switch_profile("beta")
        .await
        .expect("the request completes");
    let SwitchResponse::Switching { payloads, .. } = response else {
        panic!("an accepted switch streams, got {response:?}");
    };
    assert_eq!(
        collect_switch_events(payloads).await,
        [
            SwitchEvent::Stage {
                stage: "loading-profile".to_string()
            },
            SwitchEvent::Error {
                message: "start-local failed".to_string()
            },
        ],
        "malformed payloads are skipped; the terminal event still arrives"
    );
}

#[tokio::test]
async fn a_declined_switch_is_buffered_not_an_error() {
    let app = axum::Router::new().route(
        "/admin/switch-profile",
        axum::routing::post(|| async {
            (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {"message": "bad name", "code": "switch_failed"}
                })),
            )
        }),
    );
    let base_url = serve(app).await;
    let client = GatewayClient::new(&base_url, "").expect("client builds in tests");
    let response = client
        .switch_profile("../escape")
        .await
        .expect("a declined request still completes");
    let SwitchResponse::Buffered(answer) = response else {
        panic!("a declined switch is buffered, got {response:?}");
    };
    assert_eq!(answer.status, reqwest::StatusCode::BAD_REQUEST);
}
