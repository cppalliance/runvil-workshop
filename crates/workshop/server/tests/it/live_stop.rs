//! Workshop's server when its Gateway dies in the middle of a run.
//!
//! The one test here starts a Gateway of its own and kills it, so it is
//! kept out of the `live` module and needs two more variables than those
//! tests: the Gateway program to start, and the home folder whose
//! `.runvil/gateway.toml` it reads. That file must bind the address
//! `RUNVIL_E2E_GATEWAY` names, define a `default` profile, and serve
//! CheapSeek as its first chat model. No Gateway may be running at that
//! address when the test starts.
//!
//! ```text
//! RUNVIL_E2E_GATEWAY=http://127.0.0.1:8189/v1 RUNVIL_E2E_KEY=<key> \
//! RUNVIL_E2E_GATEWAY_BIN=<path to runvil-gateway> RUNVIL_E2E_GATEWAY_HOME=<home folder> \
//!     cargo nextest run -p workshop-server -E 'test(live_stop::)' --run-ignored ignored-only
//! ```
//!
//! The reply the test cuts off is scripted. CheapSeek says the text of a
//! `say` directive, one piece of the stream to a token, so the reply is
//! as long on a fast machine as on a slow one. A real model cannot be
//! told how long to be.
//!
//! The Gateway is killed, not asked to stop: its `POST /shutdown` lets a
//! reply in flight finish first, so a run sees no failure from it.
//!
//! Every process of the Gateway is killed at once. The program the test
//! starts only supervises: it starts a second process that serves, and
//! that one starts the model's process. With the supervisor alone killed,
//! the serving process sees that it is gone and shuts down in good order,
//! and a short reply finishes first. So the test starts the Gateway in a
//! process group of its own and kills the group, which needs `pkill` and
//! a Unix system.

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// the helpers they share; failing a test by panicking with the invariant
// named is exactly what these are for.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use workshop_run_log::{RunId, RunLog};

use crate::live::{STEP, gateway, launched, live_server};

/// Has CheapSeek stream a reply long enough to be cut off part way: a
/// `say` directive of 100,000 dots, each of which CheapSeek counts as one
/// token and sends as one piece. The Engine counts four characters to a
/// token, so the request fits a context of 32768 tokens.
const LONG_MD: &str = "---\nname: long\ndescription: Streams a long reply\nrunvil: 0\n---\n\n\
    ## Stream\n\n\
    ```lua\n\
    messages:user('[[say ' .. string.rep('.', 100000) .. ']]')\n\
    models.loop()\n\
    return messages[#messages].content\n\
    ```\n";

/// Returns at once and asks no model.
const QUIET_MD: &str = "---\nname: quiet\ndescription: Returns at once\nrunvil: 0\n---\n\n\
    ## Only\n\n\
    ```lua\nreturn 'still here'\n```\n";

/// A Gateway this test started, in a process group of its own: the
/// program that supervises, the process that serves, and the model's
/// process. All are killed when the value is dropped.
struct OwnGateway {
    /// The program the test started, which leads the group.
    program: Child,
    /// Whether the group was killed and the program reaped.
    killed: bool,
}

impl OwnGateway {
    /// Kills every process of the Gateway at once, with no chance to
    /// finish a reply.
    fn kill(&mut self) {
        // The group's id is the started program's process id. The id is
        // the program's own until the program is reaped, even after the
        // program has exited, so the group is killed first and the
        // program reaped after. From then on another process may get the
        // id, so a second call kills nothing.
        if self.killed {
            return;
        }
        self.killed = true;
        let _ = Command::new("pkill")
            .args(["-KILL", "-g", &self.program.id().to_string()])
            .status();
        let _ = self.program.wait();
    }
}

impl Drop for OwnGateway {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The first model in `listed`, a Gateway's answer to `/v1/models`, that
/// can serve a chat. Workshop picks its model by the same rule.
pub(crate) fn first_chat_model(listed: &Value) -> Option<&str> {
    listed["data"]
        .as_array()?
        .iter()
        .find(|model| workshop_menu::is_chat_capable(model))?["id"]
        .as_str()
}

/// Starts the Gateway program the environment names, in the home folder
/// it names, and waits until that Gateway answers a chat request.
async fn own_gateway(base_url: &str, key: &str) -> OwnGateway {
    let program = std::env::var("RUNVIL_E2E_GATEWAY_BIN")
        .expect("RUNVIL_E2E_GATEWAY_BIN is the Gateway program to start");
    let home = std::env::var("RUNVIL_E2E_GATEWAY_HOME")
        .expect("RUNVIL_E2E_GATEWAY_HOME is the home folder the Gateway reads");
    let client = reqwest::Client::new();
    assert!(
        client
            .get(format!("{base_url}/health"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_err(),
        "a Gateway already answers at {base_url}; stop it, since this test starts and kills its own"
    );
    let child = Command::new(program)
        .args(["--no-tray", "--profile", "default"])
        .env("HOME", &home)
        .current_dir(&home)
        // The processes the program starts stay in its group, so the
        // group is the whole Gateway.
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the Gateway program starts");
    let gateway = OwnGateway {
        program: child,
        killed: false,
    };
    let ready = json!({
        "model": "any",
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1,
    });
    tokio::time::timeout(STEP, async {
        loop {
            let listed = client
                .get(format!("{base_url}/v1/models"))
                .bearer_auth(key)
                .send()
                .await;
            if let Ok(listed) = listed
                && let Ok(body) = listed.json::<Value>().await
                && let Some(model) = first_chat_model(&body)
            {
                let mut request = ready.clone();
                request["model"] = json!(model);
                let answered = client
                    .post(format!("{base_url}/v1/chat/completions"))
                    .bearer_auth(key)
                    .json(&request)
                    .send()
                    .await;
                if answered.is_ok_and(|reply| reply.status().is_success()) {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("the started Gateway answers a chat request within the deadline");
    gateway
}

#[tokio::test]
#[ignore = "starts and kills its own Gateway; set RUNVIL_E2E_GATEWAY, RUNVIL_E2E_KEY, RUNVIL_E2E_GATEWAY_BIN and RUNVIL_E2E_GATEWAY_HOME"]
async fn a_gateway_killed_mid_run_ends_the_run_with_one_error_and_workshop_keeps_running() {
    let (base_url, key) = gateway();
    let mut own = own_gateway(&base_url, &key).await;
    let (base, dir, _state) = live_server(&[("long", LONG_MD), ("quiet", QUIET_MD)]).await;
    let mut socket = launched(&base, "long").await;
    socket
        .recv_until(STEP, |frame| {
            assert_ne!(
                frame["type"], "error",
                "no error frame may come before the reply starts: {frame}"
            );
            frame["type"] == "agent_delta"
        })
        .await;

    own.kill();

    let failed = socket
        .recv_until(STEP, |frame| frame["type"] == "error")
        .await;
    assert_eq!(
        failed["message"], "Model turn failed in agent `Stream`",
        "the client learns the round failed: {failed}"
    );

    // The first run in a fresh log is run 1. Its end is the last write.
    let log_file = dir.path().join("harness").join("runs.db");
    let row = tokio::time::timeout(STEP, async {
        loop {
            if let Ok(log) = RunLog::open(&log_file).await
                && let Ok(row) = log.run(RunId::from_raw(1)).await
                && row.result.is_some()
            {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the cut-off run is recorded as ended within the deadline");
    assert!(
        matches!(row.result, Some(Err(_))),
        "the run ends with one error: {:?}",
        row.result
    );
    socket.close().await;

    // Workshop still serves: it lists its agents, accepts a launch, and
    // says why that run cannot start. With no Gateway to list a model, no
    // run starts, even of a prompt that asks no model.
    let mut second = launched(&base, "quiet").await;
    let refused = second
        .recv_until(STEP, |frame| frame["type"] == "error")
        .await;
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("the model catalog could not be fetched")),
        "Workshop keeps running and names the missing Gateway: {refused}"
    );
    second.close().await;
}
