//! The file gates: the tools a model calls change the files of the chat's
//! run. The file tools make, edit, and delete a file, and the shell makes
//! one that the file tools read. The transcript and the model's next
//! request hold each answer, and the run log holds each file operation as
//! an effect that names its tool call. The server gives a run's
//! filesystem no policy of its own, so the default one, `AllowAll`,
//! admits each change, and no call here waits for an approval.

use std::collections::HashSet;

use workshop_run_log::{RecordFilter, RecordKind};

use super::*;
use crate::agents::Turn;

/// One call of the scripted model: a tool's wire name and its arguments.
type Call = (&'static str, serde_json::Value);

/// Streams one tool-call round attributed to `model`: the call `id` of
/// the tool with the wire name `name`, with `arguments`.
fn call_round(model: &str, id: &str, name: &str, arguments: &serde_json::Value) -> Response {
    let null = serde_json::Value::Null;
    let call = json!({ "tool_calls": [{
        "index": 0, "id": id, "type": "function",
        "function": { "name": name, "arguments": arguments.to_string() },
    }] });
    let mut sse = String::new();
    for event in [
        sse_chunk(model, &json!({ "role": "assistant" }), &null),
        sse_chunk(model, &call, &null),
        sse_chunk(model, &json!({}), &json!("tool_calls")),
    ] {
        sse.push_str(&sse_line(&event));
    }
    sse.push_str("data: [DONE]\n\n");
    ([(header::CONTENT_TYPE, "text/event-stream")], sse).into_response()
}

/// The scripted mock: the first request gets the first of `calls` as a
/// round of one tool call, the second request the second, and every
/// request after the last call streams `echo:done`. Every request body is
/// captured.
fn scripted_completions(captured: &CapturedRequests, calls: &[Call], body: &str) -> Response {
    let request: serde_json::Value = serde_json::from_str(body).expect("the request is JSON");
    let model = request["model"].as_str().unwrap_or("test-model").to_owned();
    let round = {
        let mut captured = captured.lock().expect("the capture lock is healthy");
        captured.push(request);
        captured.len() - 1
    };
    match calls.get(round) {
        Some((name, arguments)) => call_round(&model, &format!("call_{round}"), name, arguments),
        None => echo_stream(&model, "done"),
    }
}

/// Spawns the gate server over the scripted mock, with `test-model` in
/// the retained catalog and selected in the menu.
async fn spawn_scripted_server(calls: Vec<Call>) -> GateServer {
    let captured = CapturedRequests::default();
    let mock = Arc::clone(&captured);
    let calls = Arc::new(calls);
    let gateway_url = spawn_gateway(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(move |body: String| {
                    let (captured, calls) = (Arc::clone(&mock), Arc::clone(&calls));
                    async move { scripted_completions(&captured, &calls, &body) }
                }),
            )
            .route("/v1/models", typed_catalog(&["test-model"])),
    )
    .await;
    spawn_over_gateway(&gateway_url, captured, &["test-model"], Some("test-model")).await
}

/// What each tool call of `turn` answered, in order: the content of each
/// `tool_call_update` event, which is what the transcript shows.
fn tool_results(turn: &Turn) -> Vec<String> {
    turn.events
        .iter()
        .filter(|frame| frame["event"]["kind"] == "tool_call_update")
        .map(|frame| {
            frame["event"]["content"]
                .as_str()
                .expect("a tool result holds text")
                .to_owned()
        })
        .collect()
}

/// The tool message each request after the first ends with, in order:
/// what the model reads as the answer to the call of its last reply.
fn shown_answers(requests: &[serde_json::Value]) -> Vec<String> {
    requests
        .iter()
        .skip(1)
        .map(|request| {
            let last = request["messages"]
                .as_array()
                .and_then(|messages| messages.last())
                .expect("a request holds messages");
            assert_eq!(last["role"], "tool", "the request ends with a tool message");
            last["content"]
                .as_str()
                .expect("a tool message holds text")
                .to_owned()
        })
        .collect()
}

/// Sends the operator's `text` to a chat launched on `server` and returns
/// the session with the turn the scripted model's calls and reply make.
async fn run_turn(server: &GateServer, socket: &mut JsonSocket, text: &str) -> (String, Turn) {
    let session = launch_chat(socket).await;
    next_wait(socket).await;
    answer(socket, text).await;
    let turn = collect_turn(socket).await;
    assert_eq!(
        delta_text(&turn),
        "echo:done",
        "the turn ends with the reply"
    );
    assert_eq!(
        shown_answers(&server.captured.lock().expect("the capture lock is healthy")),
        tool_results(&turn),
        "the model reads each answer in its next request as the transcript shows it"
    );
    (session, turn)
}

/// GATE 15 - changing files with the file tools. The model's `Write`
/// makes a file under a directory that is absent, its `StrReplace` edits
/// the file, and its `Delete` removes it: the `Read` after the edit
/// answers the edited text, and the `ListDir` after the removal lists the
/// directory without the file. The run log holds the write as a `Vfs`
/// effect that names the `Write` call, with its answer, and every `Vfs`
/// effect of the run names a tool call.
#[tokio::test]
async fn gate_the_models_file_tool_calls_change_a_file_of_the_run() {
    const NOTE: &str = "/notes/todo.md";
    const TEXT: &str = "alpha\nbeta\n";
    let server = spawn_scripted_server(vec![
        ("Write", json!({ "path": NOTE, "contents": TEXT })),
        (
            "StrReplace",
            json!({ "path": NOTE, "old_string": "beta", "new_string": "BETA" }),
        ),
        ("Read", json!({ "path": NOTE })),
        ("Delete", json!({ "path": NOTE })),
        ("ListDir", json!({ "target_directory": "/notes" })),
    ])
    .await;
    let mut socket = connect_chat(&server.ws_base).await;
    let (session, turn) = run_turn(&server, &mut socket, "keep a note").await;

    let results = tool_results(&turn);
    assert_eq!(results.len(), 5, "one answer for each call: {results:?}");
    assert_eq!(results[0], "Wrote 11 bytes.");
    assert_eq!(results[1], "Made 1 replacement.");
    assert!(
        results[2].contains("     1|alpha\n     2|BETA\n"),
        "Read answers the edited file: {}",
        results[2]
    );
    assert_eq!(results[3], "Deleted the file.");
    assert!(
        results[4].contains("/notes/") && !results[4].contains("todo.md"),
        "ListDir lists the directory Write made, without the file Delete removed: {}",
        results[4]
    );

    let run = server
        .state
        .agents()
        .run_id(&session)
        .expect("the chat's run has begun");
    assert!(server.state.agents().close(&session), "the session closes");
    let records = closed_run_log(&server, run)
        .await
        .records(run, RecordFilter::default())
        .await
        .expect("the records read back");
    let effects: Vec<(u64, &serde_json::Value)> = records
        .iter()
        .filter(|stored| stored.record.kind == RecordKind::Effect)
        .filter_map(|stored| Some((stored.record.effect_id?, &stored.record.payload)))
        .collect();
    let calls: HashSet<u64> = effects
        .iter()
        .filter(|(_, payload)| payload.get("ToolCall").is_some())
        .map(|(id, _)| *id)
        .collect();
    let write_call = effects
        .iter()
        .find(|(_, payload)| payload.pointer("/ToolCall/tool") == Some(&json!("std/write")))
        .map(|(id, _)| *id)
        .expect("the log holds the Write call");
    let file_effects: Vec<(u64, &serde_json::Value)> = effects
        .iter()
        .filter_map(|(id, payload)| Some((*id, payload.get("Vfs")?)))
        .collect();
    for (_, asked) in &file_effects {
        assert!(
            asked["call"]
                .as_u64()
                .is_some_and(|call| calls.contains(&call)),
            "each file operation names the tool call that asked for it: {asked}"
        );
    }
    let written: Vec<&(u64, &serde_json::Value)> = file_effects
        .iter()
        .filter(|(_, asked)| asked["call"] == json!(write_call))
        .collect();
    let [(write_effect, asked)] = written.as_slice() else {
        panic!("the Write call asks for one file operation: {written:?}");
    };
    assert_eq!(
        **asked,
        json!({ "call": write_call, "op": { "Write": { "path": NOTE, "contents": TEXT } } }),
        "the effect holds the operation with the text the model wrote"
    );
    let answered = records.iter().any(|stored| {
        stored.record.kind == RecordKind::Answer
            && stored.record.effect_id == Some(*write_effect)
            && stored.record.payload.pointer("/VfsReceipt/Ok").is_some()
    });
    assert!(answered, "the log holds the answer of the write, a success");
    socket.close().await;
}

/// GATE 16 - changing files with the shell. The model's `Shell` call
/// makes a file under a directory that is absent and reads it back, and
/// the file tools read the same file, because both Packs work in the
/// files of the one run.
#[tokio::test]
async fn gate_the_models_shell_call_makes_a_file_the_file_tools_read() {
    const FILE: &str = "/work/a.txt";
    let server = spawn_scripted_server(vec![
        (
            "Shell",
            json!({ "command": format!("echo hello > {FILE}; cat {FILE}") }),
        ),
        ("Read", json!({ "path": FILE })),
    ])
    .await;
    let mut socket = connect_chat(&server.ws_base).await;
    let (_session, turn) = run_turn(&server, &mut socket, "make a file").await;

    let results = tool_results(&turn);
    assert_eq!(results.len(), 2, "one answer for each call: {results:?}");
    assert!(
        results[0].contains("Exit code: 0\n\nCommand output:\n\n```\nhello\n```"),
        "the shell writes the file and reads it back: {}",
        results[0]
    );
    assert!(
        results[1].contains("     1|hello\n"),
        "Read answers the file the shell made: {}",
        results[1]
    );
    socket.close().await;
}
