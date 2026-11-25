//! Workshop's real window, driven the way a person uses it: a message
//! typed in the chat box gets a reply in the chat panel, and closing the
//! window leaves the Gateway that Workshop started still answering.
//!
//! The tests drive the window through `tauri-driver`, Tauri's WebDriver
//! server, which needs WebKit's own driver (`WebKitWebDriver`) on Linux.
//! Workshop runs on a display and a message bus of its own, so no window
//! opens on the person's screen. Both tests are ignored unless asked for,
//! and each needs a different Gateway state, so they are asked for one at
//! a time.
//!
//! The typing test needs a Gateway that is already running:
//!
//! ```text
//! RUNVIL_E2E_GATEWAY=http://127.0.0.1:8189/v1 RUNVIL_E2E_KEY=<key> \
//! RUNVIL_E2E_GATEWAY_HOME=<home folder of the running Gateway> \
//! RUNVIL_E2E_WORKSHOP_BIN=<path to runvil-workshop> \
//!     cargo nextest run -p workshop-server -E 'test(typed_in_the_workshop_window)' --run-ignored ignored-only
//! ```
//!
//! The closing test needs no Gateway running, because Workshop starts
//! one. The Gateway program must sit beside the Workshop program, where
//! Workshop looks for it, and the home folder's `.runvil` must hold a
//! `gateway.toml` that binds the address `RUNVIL_E2E_GATEWAY` names and
//! serves a chat model. The folder also needs a `gateway.state.toml` that
//! selects a profile, with the model in it when the Gateway runs the model
//! itself: Workshop starts the Gateway without naming a profile, and with
//! no profile selected Workshop's model picker shows no model.
//!
//! ```text
//! (the same four variables)
//!     cargo nextest run -p workshop-server -E 'test(closing_workshops_window)' --run-ignored ignored-only
//! ```

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// the helpers they share; failing a test by panicking with the invariant
// named is exactly what these are for.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::time::Duration;

use serde_json::{Value, json};

use crate::live::{STEP, gateway};
use crate::live_program::{OwnSession, own_session, settings_folder, workshop_program};
use crate::live_stop::first_chat_model;

/// The chat box a person types in.
const CHAT_BOX: &str = ".ws-prompt-input__editor";

/// The chat box's round button. Its `data-action` names what Enter does:
/// `idle` while the agent's run has not started, and `send` once it has.
const ACTION_BUTTON: &str = ".ws-agent-session__action";

/// The Close button of the title bar Workshop draws.
const CLOSE_BUTTON: &str = "button.ws-window-titlebar__control[aria-label=\"Close\"]";

/// The key a WebDriver client sends for Enter.
const ENTER: &str = "\u{e007}";

/// A port nothing listens on at the moment it is picked.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port binds")
        .local_addr()
        .expect("the bound port reads back")
        .port()
}

/// The home folder the Gateway reads, which Workshop is started with too.
fn gateway_home() -> String {
    std::env::var("RUNVIL_E2E_GATEWAY_HOME")
        .expect("RUNVIL_E2E_GATEWAY_HOME is the home folder the Gateway reads")
}

/// Whether a Gateway answers at `base_url`.
async fn gateway_is_up(base_url: &str) -> bool {
    reqwest::Client::new()
        .get(format!("{base_url}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|health| health.status().is_success())
}

/// The first chat model the Gateway lists once it answers a chat request
/// with it, or `None` while it does not.
async fn answering_model(base_url: &str, key: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let listed: Value = client
        .get(format!("{base_url}/v1/models"))
        .bearer_auth(key)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let model = first_chat_model(&listed)?.to_owned();
    let reply: Value = client
        .post(format!("{base_url}/v1/chat/completions"))
        .bearer_auth(key)
        .timeout(STEP)
        .json(&json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Reply with one short greeting." }],
        }))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let text = reply["choices"][0]["message"]["content"].as_str()?;
    (!text.trim().is_empty()).then_some(model)
}

/// Waits until the Gateway answers a chat request and returns its model.
async fn model_when_answering(base_url: &str, key: &str) -> String {
    tokio::time::timeout(STEP, async {
        loop {
            if let Some(model) = answering_model(base_url, key).await {
                return model;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("the Gateway answers a chat request within the deadline")
}

/// Workshop's window under `tauri-driver`, with the session that holds
/// the driver, the display, and Workshop itself.
struct Window {
    client: reqwest::Client,
    session_url: String,
    _own: OwnSession,
    _folder: tempfile::TempDir,
}

impl Window {
    /// Starts `tauri-driver` with the home folder `home` and has it open
    /// the Workshop program's window.
    async fn open(home: &str) -> Window {
        let program = workshop_program();
        let folder = settings_folder();
        let (port, native) = (free_port(), free_port());
        let own = own_session(
            folder.path(),
            home,
            &[
                "tauri-driver",
                "--port",
                &port.to_string(),
                "--native-port",
                &native.to_string(),
            ],
        );
        let client = reqwest::Client::new();
        let driver = format!("http://127.0.0.1:{port}");
        let opened: Value = tokio::time::timeout(STEP, async {
            loop {
                let opened = client
                    .post(format!("{driver}/session"))
                    .json(&json!({ "capabilities": { "alwaysMatch": {
                        "browserName": "wry",
                        "tauri:options": { "application": program },
                    } } }))
                    .send()
                    .await;
                if let Ok(opened) = opened
                    && let Ok(body) = opened.json::<Value>().await
                {
                    return body;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        })
        .await
        .expect("tauri-driver answers within the deadline");
        let session = opened["value"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("tauri-driver opens Workshop's window: {opened}"));
        Window {
            client,
            session_url: format!("{driver}/session/{session}"),
            _own: own,
            _folder: folder,
        }
    }

    /// Sends one WebDriver command and returns its `value`. A command
    /// the driver refuses is answered with an error status and the reason
    /// in `value`, and that is an error here, as is a command that cannot
    /// be sent or answered.
    async fn command(&self, path: &str, body: Value) -> Result<Value, String> {
        let answer = self
            .client
            .post(format!("{}/{path}", self.session_url))
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("`{path}` was not answered: {error}"))?;
        let status = answer.status();
        let answer: Value = answer
            .json()
            .await
            .map_err(|error| format!("`{path}` was answered with no JSON: {error}"))?;
        if status.is_success() {
            Ok(answer["value"].clone())
        } else {
            Err(format!(
                "`{path}` was refused with {status}: {}",
                answer["value"]
            ))
        }
    }

    /// Runs `script` in the window's page and returns what it returns.
    async fn script(&self, script: &str) -> Value {
        self.command("execute/sync", json!({ "script": script, "args": [] }))
            .await
            .expect("the window runs the script")
    }

    /// Waits until `script` returns something other than null, false, or
    /// an empty string, and returns it.
    async fn when(&self, script: &str) -> Value {
        tokio::time::timeout(STEP, async {
            loop {
                let value = self.script(script).await;
                let empty = value.is_null()
                    || value == json!(false)
                    || value.as_str().is_some_and(|text| text.trim().is_empty());
                if !empty {
                    return value;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("`{script}` gives a value within the deadline"))
    }

    /// Waits until the model picker names `model`, which it does once
    /// Workshop has found the Gateway and read its model list. By then
    /// the page has loaded and its controls answer.
    async fn names_model(&self, model: &str) {
        let named = self
            .when(&format!(
                "var picker = document.querySelector('.ws-model-picker-trigger'); \
                 return picker && picker.textContent.trim() === {model:?};"
            ))
            .await;
        assert_eq!(named, json!(true));
    }

    /// The id of the one element `css` selects.
    async fn element(&self, css: &str) -> String {
        let found = self
            .command("element", json!({ "using": "css selector", "value": css }))
            .await
            .expect("the window looks for the element");
        found
            .as_object()
            .and_then(|ids| ids.values().next())
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("the window has an element for `{css}`: {found}"))
            .to_owned()
    }
}

#[tokio::test]
#[ignore = "drives the Workshop window; needs a running Gateway, tauri-driver and WebKitWebDriver; see the module's doc"]
async fn a_message_typed_in_the_workshop_window_gets_a_reply_in_the_chat_panel() {
    let (base_url, key) = gateway();
    assert!(
        gateway_is_up(&base_url).await,
        "a Gateway must be running at {base_url} before this test starts"
    );
    let model = model_when_answering(&base_url, &key).await;

    let window = Window::open(&gateway_home()).await;
    window.names_model(&model).await;
    // Enter sends nothing until the agent's run has started, so a message
    // typed before that stays in the chat box.
    let ready = window
        .when(&format!(
            "var button = document.querySelector({ACTION_BUTTON:?}); \
             return button && button.dataset.action === 'send';"
        ))
        .await;
    assert_eq!(ready, json!(true));

    let typed = "Reply with one short greeting.";
    let chat_box = window.element(CHAT_BOX).await;
    window
        .command(
            &format!("element/{chat_box}/value"),
            json!({ "text": typed }),
        )
        .await
        .expect("the chat box takes the text");
    window
        .command(
            &format!("element/{chat_box}/value"),
            json!({ "text": ENTER }),
        )
        .await
        .expect("the chat box takes Enter");

    let reply = window
        .when(
            "var reply = document.querySelector('.ws-transcript .ws-markdown-row'); \
             return reply && !document.querySelector('.ws-tail') ? reply.textContent : '';",
        )
        .await;
    let shown = window
        .script("return document.querySelector('.ws-human-message__text').textContent;")
        .await;
    assert_eq!(shown, json!(typed), "the chat panel shows what was typed");
    assert!(
        reply.as_str().is_some_and(|text| !text.trim().is_empty()),
        "the reply appears in the chat panel: {reply}"
    );
}

#[tokio::test]
#[ignore = "drives the Workshop window and has Workshop start a Gateway; needs tauri-driver and WebKitWebDriver; see the module's doc"]
async fn closing_workshops_window_leaves_the_gateway_it_started_answering() {
    let (base_url, key) = gateway();
    assert!(
        !gateway_is_up(&base_url).await,
        "a Gateway already answers at {base_url}; stop it, since in this test Workshop starts one"
    );

    let window = Window::open(&gateway_home()).await;
    let model = model_when_answering(&base_url, &key).await;
    // A click on Close does nothing while the page is still loading, and
    // a Gateway that answers at once is ready before the page is.
    window.names_model(&model).await;
    let workshop = window
        .script("return location.origin;")
        .await
        .as_str()
        .expect("the window's page has an origin")
        .to_owned();

    let close = window.element(CLOSE_BUTTON).await;
    // Workshop may be gone before it can answer the click, so the answer
    // is not waited on as a success.
    let _ = window
        .command(&format!("element/{close}/click"), json!({}))
        .await;

    tokio::time::timeout(STEP, async {
        let client = reqwest::Client::new();
        while client
            .get(&workshop)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_ok()
        {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("Workshop stops serving its page once its window is closed");

    assert!(
        answering_model(&base_url, &key).await.is_some(),
        "the Gateway Workshop started still answers a chat request after the window closed"
    );
}
