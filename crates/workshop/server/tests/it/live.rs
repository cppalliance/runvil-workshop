//! Workshop's server against a real Gateway: agents launched over the
//! agents socket make their model rounds on the Gateway the environment
//! names.
//!
//! Every test here is ignored unless asked for, because it needs a running
//! Gateway that serves a chat model:
//!
//! ```text
//! RUNVIL_E2E_GATEWAY=http://127.0.0.1:8189/v1 RUNVIL_E2E_KEY=<key> \
//!     cargo nextest run -p workshop-server -E 'test(live::)' --run-ignored ignored-only -j 1
//! ```
//!
//! `RUNVIL_E2E_GATEWAY` is the Gateway's API root and `RUNVIL_E2E_KEY` is
//! its key. The tests share that one Gateway, so `-j 1` runs them one at
//! a time.
//!
//! The chat model the tests are run on is CheapSeek
//! (<https://github.com/cppalliance/cheapseek>), a test server that
//! answers by rule, with no inference. The Gateway reaches it as a remote
//! endpoint. Each test asserts what any sensible reply satisfies and
//! nothing about its wording. The model must offer a context of at least
//! 32768 tokens, which the built-in chat agent requires, and one test
//! needs the Gateway's `trust_loopback` off.

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// the helpers they share; failing a test by panicking with the invariant
// named is exactly what these are for.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};
use workshop_server::{AgentsConfig, AppState, Config, GatewayConfig, ServerConfig};

use crate::common::{JsonSocket, launch, plugin_dir, spawn_router};

/// How long one step of a real run may take. CheapSeek answers in
/// milliseconds; the margin covers a Gateway that is still starting.
const STEP: Duration = Duration::from_secs(120);

/// The text of the Harness repository's shipped `prompts/hello.md`.
const HELLO_MD: &str = "---\nname: hello\ndescription: Say hello\nrunvil: 0\n---\n\n\
    A minimal test prompt.\n\n\
    ## Greet\n\n\
    Say \"Hello, world!\"\n\n\
    ```lua\nreturn models.infer(prose)\n```\n";

/// Drafts under the `creative` role and critiques under `strong`, one
/// model round each.
const ROLES_MD: &str = "---\nname: roles\ndescription: Drafts on one role and critiques on another\n\
    runvil: 0\n---\n\n\
    ## Draft\n\n\
    ```lua\nmodels.use('creative')\n```\n\n\
    Write a tagline for a bicycle shop.\n\n\
    ```lua\nvar.tagline = models.infer(prose)\n```\n\n\
    ## Critique\n\n\
    ```lua\nmodels.use('strong')\n```\n\n\
    Critique this tagline in one sentence: {{ var.tagline }}\n\n\
    ```lua\nreturn models.infer(prose)\n```\n";

/// Offers the model one adhoc tool and asks it to use that tool. The run
/// returns whether the tool's handler ran, a bar, and the model's last
/// message.
const GRAB_MD: &str = "---\nname: grab\ndescription: Offers a Lua tool to the model\n\
    runvil: 0\n---\n\n\
    ## Ask\n\n\
    ```lua\n\
    local grab = tools.adhoc({\n\
    \x20 name = 'grab',\n\
    \x20 description = 'Grab a value',\n\
    \x20 parameters = { value = 'string' },\n\
    }, function(args)\n\
    \x20 var.grabbed = args.value\n\
    \x20 return 'got ' .. args.value\n\
    end)\n\
    tools.offer(grab)\n\
    messages:user('Call the grab tool with the value b. Then tell me what it returned.')\n\
    models.loop()\n\
    return tostring(var.grabbed) .. '|' .. messages[#messages].content\n\
    ```\n";

/// A larger prompt: an argument with a default, the store, a tool its Lua
/// calls, and a fanout of two tasks that each make a model round, before
/// a last round over what they wrote.
const REPORT_MD: &str = "---\nname: report\ndescription: Reports on a topic from two notes\n\
    runvil: 0\n\
    args:\n\
    \x20 type: object\n\
    \x20 properties:\n\
    \x20   topic: { type: string, default: tea, description: What to report on }\n\
    ---\n\n\
    ```lua\n\
    store.write('notes.md', '# Notes on ' .. args.topic .. '\\n')\n\
    ```\n\n\
    ## Gather\n\n\
    ```lua\n\
    local count = tools.adhoc({\n\
    \x20 name = 'count',\n\
    \x20 description = 'Count the letters of a word',\n\
    \x20 parameters = { word = 'string' },\n\
    }, function(a)\n\
    \x20 return tostring(#a.word)\n\
    end)\n\
    store.append('notes.md', 'letters: ' .. tools.call(count, { word = args.topic }) .. '\\n')\n\
    local notes = tasks.fanout('Note', { 'history', 'uses' })\n\
    for _, note in ipairs(notes) do\n\
    \x20 store.append('notes.md', '- ' .. note.text .. '\\n')\n\
    end\n\
    ```\n\n\
    ### Note\n\n\
    ```lua\n\
    return models.infer('Write one short sentence about the ' .. item .. ' of ' .. args.topic .. '.')\n\
    ```\n\n\
    ## Report\n\n\
    ```lua\n\
    var.notes = store.read('notes.md')\n\
    ```\n\n\
    Summarize these notes in one sentence:\n\n\
    {{ var.notes }}\n\n\
    ```lua\n\
    return var.notes .. '=====\\n' .. models.infer(prose)\n\
    ```\n";

/// The Gateway the environment names: its base URL with no path, and its
/// key.
fn gateway() -> (String, String) {
    let root = std::env::var("RUNVIL_E2E_GATEWAY").expect("RUNVIL_E2E_GATEWAY is the API root");
    let key = std::env::var("RUNVIL_E2E_KEY").expect("RUNVIL_E2E_KEY is the Gateway's key");
    let base = root.trim_end_matches('/').trim_end_matches("/v1");
    (base.to_owned(), key)
}

/// A Workshop server bound to the Gateway the environment names, with
/// each of `agents` written to its agents folder as `<name>.md`, and its
/// catalog holding the models that Gateway lists.
async fn live_server(agents: &[(&str, &str)]) -> (String, tempfile::TempDir, AppState) {
    let (base_url, api_key) = gateway();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let agents_dir = dir.path().join("agents");
    std::fs::create_dir(&agents_dir).expect("the agents directory creates");
    for (name, text) in agents {
        std::fs::write(agents_dir.join(format!("{name}.md")), text).expect("the agent writes");
    }
    let config = Config {
        gateway: GatewayConfig {
            base_url: base_url.clone(),
            api_key: api_key.clone(),
        },
        server: ServerConfig {
            state_dir: dir.path().to_path_buf(),
            ..ServerConfig::default()
        },
        agents: AgentsConfig {
            path: agents_dir,
            plugins: Some(plugin_dir()),
            ..AgentsConfig::default()
        },
    };
    let (state, base) = spawn_router(&config).await;
    // The router is bound without the serving loop, which is what fetches
    // the catalog in the product, so the test fetches it the same way.
    let listed: Value = reqwest::Client::new()
        .get(format!("{base_url}/v1/models"))
        .bearer_auth(&api_key)
        .send()
        .await
        .expect("the Gateway answers its model list")
        .json()
        .await
        .expect("the model list is JSON");
    let models = listed["data"]
        .as_array()
        .expect("the model list holds a `data` array")
        .clone();
    assert!(!models.is_empty(), "the Gateway serves at least one model");
    state.catalog().publish(models);
    (base, dir, state)
}

/// Connects to the agents socket and launches `agent`.
async fn launched(base: &str, agent: &str) -> JsonSocket {
    let mut socket = JsonSocket::connect(&format!("{base}/agents/ws")).await;
    assert_eq!(
        socket.recv_json().await["type"],
        "agents",
        "the connect-time push lists the agents"
    );
    let _session = launch(&mut socket, agent).await;
    socket
}

/// Receives frames until the event of kind `kind`, refusing an error
/// frame on the way, and returns that event's frame.
async fn event_of(socket: &mut JsonSocket, kind: &str) -> Value {
    socket
        .recv_until(STEP, |frame| {
            assert_ne!(
                frame["type"], "error",
                "no error frame may come before `{kind}`: {frame}"
            );
            frame["type"] == "agent_event" && frame["event"]["kind"] == kind
        })
        .await
}

/// Receives every event frame up to the run's end, refusing an error
/// frame on the way, and returns their events in order.
async fn events_to_the_end(socket: &mut JsonSocket) -> Vec<Value> {
    let events = Mutex::new(Vec::new());
    socket
        .recv_until(STEP, |frame| {
            assert_ne!(
                frame["type"], "error",
                "no error frame may come before the run's end: {frame}"
            );
            if frame["type"] != "agent_event" {
                return false;
            }
            events
                .lock()
                .expect("the event list is healthy")
                .push(frame["event"].clone());
            frame["event"]["kind"] == "run_ended"
        })
        .await;
    events.into_inner().expect("the event list is healthy")
}

/// The contents of the events of kind `kind`, in order.
fn contents<'a>(events: &'a [Value], kind: &str) -> Vec<&'a str> {
    events
        .iter()
        .filter(|event| event["kind"] == kind)
        .map(|event| event["content"].as_str().unwrap_or_default())
        .collect()
}

/// Sends the operator's message `text`.
async fn say(socket: &mut JsonSocket, text: &str) {
    socket
        .send_json(&json!({ "type": "input_response", "text": text }))
        .await;
}

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn the_shipped_hello_prompt_runs_through_workshop_and_ends_with_text() {
    let (base, _dir, _state) = live_server(&[("hello", HELLO_MD)]).await;
    let mut socket = launched(&base, "hello").await;

    let reply = event_of(&mut socket, "agent_message").await;
    assert!(
        reply["event"]["content"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty()),
        "the model's reply reaches the client as text: {reply}"
    );
    let ended = event_of(&mut socket, "run_ended").await;
    assert_eq!(ended["event"]["section"], "hello", "{ended}");
    socket.close().await;
}

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn the_chat_agent_recalls_a_name_from_an_earlier_message() {
    let (base, _dir, _state) = live_server(&[]).await;
    let mut socket = launched(&base, "chat").await;

    event_of(&mut socket, "awaiting_input").await;
    say(&mut socket, "Hi, I'm Sam.").await;
    event_of(&mut socket, "agent_message").await;
    event_of(&mut socket, "awaiting_input").await;
    say(&mut socket, "What is my name?").await;
    let second = event_of(&mut socket, "agent_message").await;
    let text = second["event"]["content"].as_str().unwrap_or_default();
    assert!(
        text.contains("Sam"),
        "the second reply holds the name from the first message: {second}"
    );
    socket.close().await;
}

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn a_prompt_that_drafts_on_one_role_and_critiques_on_another_gets_two_replies() {
    let (base, _dir, _state) = live_server(&[("roles", ROLES_MD)]).await;
    let mut socket = launched(&base, "roles").await;

    let events = events_to_the_end(&mut socket).await;
    let replies = contents(&events, "agent_message");
    assert_eq!(replies.len(), 2, "one reply per round: {events:?}");
    assert!(
        replies.iter().all(|reply| !reply.trim().is_empty()),
        "both rounds get a reply: {replies:?}"
    );
    socket.close().await;
}

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn a_model_calls_an_offered_tool_and_the_round_after_it_succeeds() {
    let (base, _dir, _state) = live_server(&[("grab", GRAB_MD)]).await;
    let mut socket = launched(&base, "grab").await;

    let events = events_to_the_end(&mut socket).await;
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event["kind"].as_str().unwrap_or_default())
        .collect();
    let first = |wanted: &str| kinds.iter().position(|kind| *kind == wanted);
    let (called, answered) = (first("tool_call"), first("tool_call_update"));
    let replied = kinds.iter().rposition(|kind| *kind == "agent_message");
    assert!(
        called.is_some() && called < answered && answered < replied,
        "the model calls the tool, the tool answers, and a reply follows: {kinds:?}"
    );
    let last = contents(&events, "agent_message").pop().unwrap_or_default();
    assert!(!last.trim().is_empty(), "the round after the call replies");
    socket.close().await;
}

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn a_larger_prompt_with_an_argument_the_store_a_tool_and_tasks_ends_with_text() {
    let (base, _dir, _state) = live_server(&[("report", REPORT_MD)]).await;
    let mut socket = launched(&base, "report").await;

    let events = events_to_the_end(&mut socket).await;
    let replies = contents(&events, "agent_message");
    assert_eq!(
        replies.len(),
        3,
        "two notes and one summary, a model round each: {events:?}"
    );
    assert!(
        replies.iter().all(|reply| !reply.trim().is_empty()),
        "every round gets a reply: {replies:?}"
    );
    socket.close().await;
}

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model and `trust_loopback = false`; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn a_keyless_request_is_refused_and_workshops_run_still_ends_with_text() {
    let (base_url, _key) = gateway();
    let keyless = reqwest::Client::new()
        .post(format!("{base_url}/v1/chat/completions"))
        .json(&json!({
            "model": "any",
            "messages": [{ "role": "user", "content": "ping" }],
        }))
        .send()
        .await
        .expect("the Gateway answers the keyless request");
    assert_eq!(
        keyless.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "a request with no key is refused; this test needs a Gateway whose `trust_loopback` is false"
    );

    let (base, _dir, _state) = live_server(&[("hello", HELLO_MD)]).await;
    let mut socket = launched(&base, "hello").await;
    let reply = event_of(&mut socket, "agent_message").await;
    assert!(
        reply["event"]["content"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty()),
        "Workshop sends its key, so its run still ends with text: {reply}"
    );
    event_of(&mut socket, "run_ended").await;
    socket.close().await;
}
