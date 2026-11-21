//! The Workshop program started beside a running Gateway: it finds that
//! Gateway through the Gateway's discovery file, reports its address, and
//! a chat with the built-in agent gets a reply.
//!
//! The one test here starts the program itself, on a display of its own
//! and a message bus of its own, so that no window opens on the person's
//! screen and the program cannot hand over to a Workshop the person has
//! open. It needs `setsid`, `xvfb-run`, `dbus-run-session` and `lsof`, so
//! it runs on Linux only. It is ignored unless asked for:
//!
//! ```text
//! RUNVIL_E2E_GATEWAY=http://127.0.0.1:8189/v1 \
//! RUNVIL_E2E_GATEWAY_HOME=<home folder of the running Gateway> \
//! RUNVIL_E2E_WORKSHOP_BIN=<path to runvil-workshop> \
//!     cargo nextest run -p workshop-server -E 'test(live_program::)' --run-ignored ignored-only
//! ```
//!
//! `RUNVIL_E2E_GATEWAY_HOME` is the home folder the running Gateway was
//! started with. The program is started with the same one, since that is
//! where it looks for the Gateway's discovery file.

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// the helpers they share; failing a test by panicking with the invariant
// named is exactly what these are for.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use crate::common::{JsonSocket, plugin_dir};
use crate::live::STEP;

/// The Workshop program this test started, in a session of its own with
/// every process it starts. All of them are killed when the value is
/// dropped.
struct OwnWorkshop(Child);

impl OwnWorkshop {
    /// The id of the session the program leads, which is its process id.
    fn session(&self) -> String {
        self.0.id().to_string()
    }

    /// The ids of the processes in the program's session.
    fn processes(&self) -> Vec<String> {
        let listed = Command::new("pgrep")
            .args(["-s", &self.session()])
            .output()
            .expect("pgrep runs");
        String::from_utf8_lossy(&listed.stdout)
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    /// The loopback ports any of the program's processes listens on.
    fn loopback_ports(&self) -> Vec<u16> {
        let processes = self.processes();
        if processes.is_empty() {
            return Vec::new();
        }
        let listed = Command::new("lsof")
            .args(["-nP", "-a", "-p", &processes.join(",")])
            .args(["-iTCP", "-sTCP:LISTEN", "-Fn"])
            .output()
            .expect("lsof runs");
        String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix("n127.0.0.1:"))
            .filter_map(|port| port.parse().ok())
            .collect()
    }
}

impl Drop for OwnWorkshop {
    fn drop(&mut self) {
        // The program runs under three wrappers, and its message bus
        // starts helpers of its own, so the whole session goes.
        let _ = Command::new("pkill")
            .args(["-KILL", "-s", &self.session()])
            .status();
        let _ = self.0.wait();
        // A killed process takes a moment to go. Waiting here means the
        // test leaves nothing running behind it.
        for _ in 0..50 {
            if self.processes().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Starts the Workshop program the environment names in `folder`, with
/// the home folder `home`, in a session of its own, on a display and a
/// message bus of its own.
fn own_workshop(folder: &Path, home: &str) -> OwnWorkshop {
    let program = std::env::var("RUNVIL_E2E_WORKSHOP_BIN")
        .expect("RUNVIL_E2E_WORKSHOP_BIN is the Workshop program to start");
    let log = |name: &str| {
        Stdio::from(std::fs::File::create(folder.join(name)).expect("the log file creates"))
    };
    // `setsid` makes the child the leader of a new session, so the
    // session's id is the child's process id.
    let child = Command::new("setsid")
        .args(["dbus-run-session", "--", "xvfb-run", "-a", &program])
        .env("HOME", home)
        // With these two, the program can only draw on the display that
        // `xvfb-run` makes, never on the person's own session.
        .env_remove("WAYLAND_DISPLAY")
        .env("GDK_BACKEND", "x11")
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(log("workshop.stdout.log"))
        .stderr(log("workshop.stderr.log"))
        .spawn()
        .expect("the Workshop program starts");
    OwnWorkshop(child)
}

/// The loopback port on which the program serves Workshop's page.
async fn served_port(workshop: &OwnWorkshop) -> u16 {
    let client = reqwest::Client::new();
    tokio::time::timeout(STEP, async {
        loop {
            for port in workshop.loopback_ports() {
                let page = client
                    .get(format!("http://127.0.0.1:{port}/"))
                    .timeout(Duration::from_secs(2))
                    .send()
                    .await;
                if let Ok(page) = page
                    && let Ok(text) = page.text().await
                    && text.contains("<title>Runvil</title>")
                {
                    return port;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("the program serves Workshop's page on a loopback port within the deadline")
}

#[tokio::test]
#[ignore = "starts the Workshop program; needs a running Gateway; set RUNVIL_E2E_GATEWAY, RUNVIL_E2E_GATEWAY_HOME and RUNVIL_E2E_WORKSHOP_BIN"]
async fn the_workshop_program_reports_the_running_gateways_address_and_its_chat_replies() {
    let root = std::env::var("RUNVIL_E2E_GATEWAY").expect("RUNVIL_E2E_GATEWAY is the API root");
    let gateway = root
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_owned();
    let home = std::env::var("RUNVIL_E2E_GATEWAY_HOME")
        .expect("RUNVIL_E2E_GATEWAY_HOME is the running Gateway's home folder");
    let client = reqwest::Client::new();
    assert!(
        client
            .get(format!("{gateway}/health"))
            .send()
            .await
            .is_ok_and(|health| health.status().is_success()),
        "a Gateway must be running at {gateway} before this test starts"
    );

    // The program reads `workshop.toml` from the folder it starts in, and
    // keeps its state there. The file must name a Gateway; this one names
    // an address nothing listens on, so the address the program reports
    // can only have come from the running Gateway's discovery file.
    let folder = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        folder.path().join("workshop.toml"),
        format!(
            "[gateway]\nbase_url = \"http://127.0.0.1:1\"\napi_key = \"unused\"\n\n\
             [agents]\nplugins = {:?}\n",
            plugin_dir().display().to_string()
        ),
    )
    .expect("the settings file writes");
    let workshop = own_workshop(folder.path(), &home);
    let port = served_port(&workshop).await;

    let reported = tokio::time::timeout(STEP, async {
        loop {
            let origin = client
                .get(format!("http://127.0.0.1:{port}/gateway/origin"))
                .send()
                .await;
            if let Ok(origin) = origin
                && let Ok(body) = origin.json::<Value>().await
                && body["origin"] == gateway.as_str()
            {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("Workshop reports the running Gateway's address within the deadline");
    assert_eq!(reported["origin"], gateway.as_str());

    let mut socket = JsonSocket::connect(&format!("ws://127.0.0.1:{port}/agents/ws")).await;
    assert_eq!(socket.recv_json().await["type"], "agents");
    socket
        .send_json(&json!({ "type": "launch", "agent": "chat" }))
        .await;
    let event = |kind: &'static str| {
        move |frame: &Value| {
            assert_ne!(
                frame["type"], "error",
                "no error frame may come before `{kind}`: {frame}"
            );
            frame["type"] == "agent_event" && frame["event"]["kind"] == kind
        }
    };
    socket.recv_until(STEP, event("awaiting_input")).await;
    socket
        .send_json(&json!({ "type": "input_response", "text": "Reply with one short greeting." }))
        .await;
    let reply = socket.recv_until(STEP, event("agent_message")).await;
    assert!(
        reply["event"]["content"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty()),
        "the chat's run gets a reply through the Gateway the program found: {reply}"
    );
    socket.close().await;
}
