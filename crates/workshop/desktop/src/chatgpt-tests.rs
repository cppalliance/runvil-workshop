use workshop_server_api::ChatgptAccount;

use super::{
    AccountView, CapabilityGate, EnableModelsReply, WAITING_DETAIL, enable_models_for,
    enable_outcome, enable_reply_from_body, ipc_json, ipc_json_leaks, reset_view_for,
    sign_in_view_for, status_view_for, view_of_status,
};

fn status(value: &serde_json::Value) -> AccountView {
    view_of_status(value)
}

#[test]
fn status_views_cover_the_screen_states() {
    let waiting = status(&serde_json::json!({
        "status": null,
        "sign_in_pending": true,
        "available": true,
        "email": "a@example.com"
    }));
    assert_eq!(waiting.state, "waiting");
    assert_eq!(waiting.detail, WAITING_DETAIL);
    assert!(
        waiting
            .detail
            .contains("Quitting Workshop stops the Gateway")
    );

    let signed_in = status(&serde_json::json!({
        "status": "active",
        "plan_granted": true,
        "available": true,
        "email": "a@example.com"
    }));
    assert_eq!(signed_in.state, "signed_in");
    assert!(signed_in.plan_granted);

    let needs = status(&serde_json::json!({
        "status": "needs_auth",
        "available": true
    }));
    assert_eq!(needs.state, "needs_sign_in");

    let unread = status(&serde_json::json!({ "unreadable": true, "available": true }));
    assert_eq!(unread.state, "unreadable");

    let down = status(&serde_json::json!({
        "available": false,
        "unavailable_reason": "Windows"
    }));
    assert_eq!(down.state, "unavailable");
    assert_eq!(down.detail, "Windows");
}

#[test]
fn ipc_payloads_carry_no_secret() {
    let view = status(&serde_json::json!({
        "status": "active",
        "plan_granted": true,
        "available": true,
        "email": "a@example.com",
        "authorize_url": "http://secret.example/callback",
        "owner_proof": "owner-proof-should-not-pass",
        "api_key": "bearer-should-not-pass"
    }));
    let json = ipc_json(&view);
    assert!(!ipc_json_leaks(&json), "{json}");
    assert!(!json.contains("secret.example"), "{json}");
    assert!(!json.contains("owner-proof"), "{json}");
    assert!(!json.contains("bearer-should"), "{json}");

    let reply = enable_reply_from_body(
        r#"{"endpoint":"chatgpt-plan","added":["gpt"],"renamed":[{"from":"gpt","to":"gpt-chatgpt"}],"skipped":[{"name":"broken-model","reason":"missing_context_window"}],"authorize_url":"http://nope","access_token":"tok"}"#,
    )
    .expect("enable reply");
    let enable_json = ipc_json(&reply);
    assert!(!ipc_json_leaks(&enable_json), "{enable_json}");
    assert_eq!(
        reply,
        EnableModelsReply {
            added: vec!["gpt".to_owned()],
            renamed: vec![super::RenamedModel {
                from: "gpt".to_owned(),
                to: "gpt-chatgpt".to_owned(),
            }],
            skipped: vec![super::SkippedModel {
                name: "broken-model".to_owned(),
                reason: "missing_context_window".to_owned(),
            }],
            endpoint: "chatgpt-plan".to_owned(),
        }
    );
}

#[tokio::test]
async fn the_authorize_url_is_passed_to_the_opener_only_and_excluded_from_ipc() {
    let (dir, server) = spawn_fake_gateway_sequence(&[
        (
            200,
            r#"{"authorize_url":"https://auth.example/go?state=synth-state"}"#,
        ),
        (
            200,
            r#"{"status":null,"sign_in_pending":true,"available":true,"authorize_url":"https://auth.example/go?state=synth-state","owner_proof":"synth-owner"}"#,
        ),
    ]);
    let mut opened = None;
    let view = sign_in_view_for(&ChatgptAccount::new(dir.path()), |url| {
        opened = Some(url.to_owned());
        Ok::<(), &'static str>(())
    })
    .await;
    let _ = server.join();
    assert_eq!(
        opened.as_deref(),
        Some("https://auth.example/go?state=synth-state")
    );
    assert_eq!(view.state, "waiting");
    let json = ipc_json(&view);
    assert_no_secret_values(&json, &["auth.example", "synth-state", "synth-owner"]);
}

#[test]
fn a_conflict_envelope_is_not_an_empty_enable() {
    let body = r#"{"error":{"message":"secret-draft-path","type":"invalid_request_error","code":"config_has_pending_changes"}}"#;
    let parsed = enable_reply_from_body(body).expect("defaulted fields accept the envelope");
    assert!(
        parsed.added.is_empty(),
        "the old parser treats the envelope as success"
    );
    let err = enable_outcome(409, body).expect_err("a 409 is not a successful enable");
    assert_eq!(err, "config_has_pending_changes");
    assert!(!err.contains("secret-draft-path"), "{err}");
    let denied = enable_outcome(
        403,
        r#"{"error":{"message":"owner proof","type":"auth","code":"owner_proof_required"}}"#,
    )
    .expect_err("a 403 is not a successful enable");
    assert_eq!(denied, "owner_proof_required");
    let ok = enable_outcome(
        200,
        r#"{"endpoint":"chatgpt-plan","added":["gpt"],"renamed":[],"skipped":[]}"#,
    )
    .expect("a 200 list is the enable reply");
    assert_eq!(ok.added, vec!["gpt".to_owned()]);
}

#[test]
fn the_capability_binds_once() {
    let mut gate = CapabilityGate::default();
    assert!(gate.bind("").is_err());
    gate.bind("capability-token-1").expect("first bind");
    assert!(gate.admit("capability-token-1"));
    assert!(!gate.admit("capability-token-2"));
    assert!(gate.bind("capability-token-2").is_err());
    assert!(gate.admit("capability-token-1"));
}

#[test]
fn the_capability_gate_resets_for_main_reload_and_allows_rebind() {
    let mut gate = CapabilityGate::default();
    gate.bind("capability-token-1").expect("first bind");
    assert!(gate.admit("capability-token-1"));

    gate.reset_for_main_load();
    assert!(!gate.admit("capability-token-1"));
    assert!(!gate.admit("capability-token-2"));

    gate.bind("capability-token-2")
        .expect("rebind after main load");
    assert!(gate.admit("capability-token-2"));
    assert!(!gate.admit("capability-token-1"));
    assert!(
        gate.bind("capability-token-3").is_err(),
        "refused by CapabilityGate: second bind within reloaded document must fail"
    );
}

/// `enable_models_for` itself, against a local fake Gateway. A direct call to
/// `enable_outcome` would still pass if this call site went back to treating
/// an error envelope as an empty success.
#[tokio::test]
async fn enable_models_returns_the_gateway_code() {
    let conflict = r#"{"error":{"message":"secret-draft-path","type":"invalid_request_error","code":"config_has_pending_changes"}}"#;
    let conflicted = enable_against(409, conflict)
        .await
        .expect_err("a 409 is not a successful enable");
    assert_eq!(conflicted, "config_has_pending_changes");
    assert!(!conflicted.contains("secret-draft-path"));

    let envelope = r#"{"error":{"message":"secret-draft-path","type":"invalid_request_error","code":"owner_proof_required"}}"#;
    let denied = enable_against(200, envelope)
        .await
        .expect_err("a 200 error envelope is not a successful enable");
    assert_eq!(denied, "owner_proof_required");
    assert!(!denied.contains("secret-draft-path"));

    let long = "a".repeat(200);
    let body = format!(
        r#"{{"error":{{"message":"secret-draft-path","type":"invalid_request_error","code":"{long}"}}}}"#
    );
    let bounded = enable_against(409, &body)
        .await
        .expect_err("a long code is not returned whole");
    assert_eq!(bounded, "gateway_error");
    assert!(!bounded.contains(&long));
}

async fn enable_against(status: u16, body: &str) -> Result<super::EnableModelsReply, String> {
    let (dir, server) = spawn_fake_gateway(status, body);
    let result = enable_models_for(&ChatgptAccount::new(dir.path())).await;
    let _ = server.join();
    result
}

fn spawn_fake_gateway(status: u16, body: &str) -> (tempfile::TempDir, std::thread::JoinHandle<()>) {
    spawn_fake_gateway_sequence(&[(status, body)])
}

fn spawn_fake_gateway_sequence(
    replies: &[(u16, &str)],
) -> (tempfile::TempDir, std::thread::JoinHandle<()>) {
    use std::io::{Read as _, Write as _};
    use workshop_server_api::discovery::{GatewayDiscoveryFile, GatewayPrivateFile};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the fake gateway binds");
    let port = listener
        .local_addr()
        .expect("the fake gateway has a port")
        .port();
    let owned_replies: Vec<(u16, String)> = replies
        .iter()
        .map(|(status, body)| (*status, (*body).to_owned()))
        .collect();
    let server = std::thread::spawn(move || {
        for (status, payload) in owned_replies {
            let (mut sock, _) = listener.accept().expect("the gateway request arrives");
            let mut buf = [0_u8; 8192];
            let mut filled = 0;
            while filled < buf.len() {
                let read = sock.read(&mut buf[filled..]).unwrap_or(0);
                if read == 0 {
                    break;
                }
                filled += read;
                if buf[..filled].windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = sock.write_all(header.as_bytes());
            let _ = sock.write_all(payload.as_bytes());
        }
    });
    let dir = tempfile::tempdir().expect("a temp run dir");
    let discovery = GatewayDiscoveryFile {
        port,
        api_key: "test-key".to_owned(),
        pid: 7,
        epoch: 9,
        version: "test".to_owned(),
        started_at: "1970-01-01T00:00:09Z".to_owned(),
    };
    let private = GatewayPrivateFile {
        pid: 7,
        epoch: 9,
        plan_proof: "planproof".to_owned(),
        owner_proof: "ownerproof".to_owned(),
    };
    std::fs::write(
        dir.path().join("gateway.json"),
        serde_json::to_vec(&discovery).expect("discovery json"),
    )
    .expect("write discovery");
    std::fs::write(
        dir.path().join("gateway-private.json"),
        serde_json::to_vec(&private).expect("private json"),
    )
    .expect("write private");
    (dir, server)
}

#[test]
fn capability_errors_name_unmet_condition_without_secrets() {
    let mut gate = CapabilityGate::default();
    let unbound = gate
        .verify("secret-cap-1")
        .expect_err("unbound gate must refuse");
    assert!(unbound.contains("none is bound"), "{unbound}");
    assert!(!unbound.contains("secret-cap-1"), "{unbound}");

    let empty = gate.bind("").expect_err("empty bind must fail");
    assert!(empty.contains("non-empty token"), "{empty}");

    gate.bind("secret-cap-1").expect("first bind");
    let second = gate
        .bind("secret-cap-2")
        .expect_err("second bind must fail");
    assert!(second.contains("already holds a capability"), "{second}");
    assert!(!second.contains("secret-cap-1"), "{second}");
    assert!(!second.contains("secret-cap-2"), "{second}");

    let mismatch = gate
        .verify("wrong-cap")
        .expect_err("mismatching token must fail");
    assert!(mismatch.contains("did not match"), "{mismatch}");
    assert!(!mismatch.contains("secret-cap-1"), "{mismatch}");
    assert!(!mismatch.contains("wrong-cap"), "{mismatch}");
}

#[tokio::test]
async fn gateway_errors_report_cause_without_swallowing_or_leaking() {
    let empty_dir = tempfile::tempdir().expect("temp run dir");
    let missing = status_view_for(&ChatgptAccount::new(empty_dir.path())).await;
    assert_eq!(missing.state, "unavailable");
    assert!(
        missing.detail.contains("discovery file"),
        "{}",
        missing.detail
    );
    assert!(missing.detail.contains("none found"), "{}", missing.detail);
    assert!(
        !missing.detail.contains("not running"),
        "{}",
        missing.detail
    );

    let (status_dir, status_server) = spawn_fake_gateway(503, r#"{"error":"busy"}"#);
    let status_err = status_view_for(&ChatgptAccount::new(status_dir.path())).await;
    let _ = status_server.join();
    assert_eq!(status_err.state, "unavailable");
    assert!(
        status_err.detail.contains("HTTP 503"),
        "{}",
        status_err.detail
    );
    assert!(
        status_err.detail.contains("required 2xx"),
        "{}",
        status_err.detail
    );
    assert!(
        !status_err.detail.contains("test-key"),
        "{}",
        status_err.detail
    );
    assert!(
        !status_err.detail.contains("ownerproof"),
        "{}",
        status_err.detail
    );

    let (post_dir, post_server) = spawn_fake_gateway(500, r#"{"error":"boom"}"#);
    let post_err = reset_view_for(&ChatgptAccount::new(post_dir.path())).await;
    let _ = post_server.join();
    assert_eq!(post_err.state, "unavailable");
    assert!(post_err.detail.contains("HTTP 500"), "{}", post_err.detail);
    assert!(!post_err.detail.contains("test-key"), "{}", post_err.detail);
    assert!(
        !post_err.detail.contains("ownerproof"),
        "{}",
        post_err.detail
    );

    let (open_dir, open_server) = spawn_fake_gateway(
        200,
        r#"{"authorize_url":"https://auth.example/secret-token"}"#,
    );
    let open_err = sign_in_view_for(&ChatgptAccount::new(open_dir.path()), |url| {
        Err(format!("launch failed for {url}"))
    })
    .await;
    let _ = open_server.join();
    assert_eq!(open_err.state, "unavailable");
    assert!(
        open_err.detail.contains("system browser failed"),
        "{}",
        open_err.detail
    );
    assert!(
        !open_err.detail.contains("launch failed"),
        "{}",
        open_err.detail
    );
    assert!(
        !open_err.detail.contains("auth.example"),
        "{}",
        open_err.detail
    );
    assert!(
        !open_err.detail.contains("secret-token"),
        "{}",
        open_err.detail
    );
}

fn assert_no_secret_values(json: &str, forbidden: &[&str]) {
    assert!(!ipc_json_leaks(json), "field-name leak in {json}");
    for secret in forbidden {
        assert!(
            !json.contains(secret),
            "synthetic secret {secret:?} leaked into IPC payload: {json}"
        );
    }
}

#[tokio::test]
async fn wrong_typed_status_and_sidecar_inputs_never_echo_values_across_ipc() {
    use workshop_server_api::discovery::{GatewayDiscoveryFile, GatewayPrivateFile};

    const OWNER_SECRET: &str = "synth-owner-proof-9f8e7d";
    const PLAN_SECRET: &str = "synth-plan-proof-6c5b4a";
    const STATE_SECRET: &str = "synth-oauth-state-3a2b1c";
    const BEARER_SECRET: &str = "synth-api-bearer-4d3c2b";
    let forbidden = [OWNER_SECRET, PLAN_SECRET, STATE_SECRET, BEARER_SECRET];

    // 1. Wrong-typed status fields: `serde_json`'s data error can echo a string value.
    let wrong_status = status(&serde_json::json!({
        "available": OWNER_SECRET,
        "plan_granted": PLAN_SECRET,
        "sign_in_pending": STATE_SECRET
    }));
    assert_eq!(wrong_status.state, "unreadable");
    assert_no_secret_values(&ipc_json(&wrong_status), &forbidden);

    let (bad_status_dir, bad_status_server) = spawn_fake_gateway(
        200,
        &format!(r#"{{"available":"{OWNER_SECRET}","plan_granted":"{PLAN_SECRET}"}}"#),
    );
    let fetched_wrong_status = status_view_for(&ChatgptAccount::new(bad_status_dir.path())).await;
    let _ = bad_status_server.join();
    assert_eq!(fetched_wrong_status.state, "unreadable");
    assert_no_secret_values(&ipc_json(&fetched_wrong_status), &forbidden);

    let enable_err = enable_reply_from_body(&format!(r#"{{"added":"{OWNER_SECRET}"}}"#))
        .expect_err("wrong-typed added list must fail");
    assert_no_secret_values(&enable_err, &forbidden);

    // 2. Wrong-typed gateway-private.json and gateway.json sidecars (neither values nor paths leak).
    let sidecar_dir = tempfile::tempdir().expect("temp run dir");
    let dir_str = sidecar_dir.path().to_string_lossy().into_owned();
    let discovery = GatewayDiscoveryFile {
        port: 43123,
        api_key: BEARER_SECRET.to_owned(),
        pid: 7,
        epoch: 9,
        version: "test".to_owned(),
        started_at: "1970-01-01T00:00:09Z".to_owned(),
    };
    std::fs::write(
        sidecar_dir.path().join("gateway.json"),
        serde_json::to_vec(&discovery).expect("discovery json"),
    )
    .expect("write discovery");
    std::fs::write(
        sidecar_dir.path().join("gateway-private.json"),
        format!(
            r#"{{"pid":"{OWNER_SECRET}","epoch":"{PLAN_SECRET}","plan_proof":"{PLAN_SECRET}","owner_proof":"{OWNER_SECRET}"}}"#
        ),
    )
    .expect("write wrong-typed private file");

    let private_parse_view = status_view_for(&ChatgptAccount::new(sidecar_dir.path())).await;
    assert_eq!(private_parse_view.state, "unreadable");
    let private_json = ipc_json(&private_parse_view);
    assert_no_secret_values(&private_json, &forbidden);
    assert!(
        !private_json.contains(&dir_str),
        "run path leaked: {private_json}"
    );

    // Boot mismatch reports (pid, epoch) rather than an invented boot_id and never echoes proofs.
    let mismatched_private = GatewayPrivateFile {
        pid: 11,
        epoch: 13,
        plan_proof: PLAN_SECRET.to_owned(),
        owner_proof: OWNER_SECRET.to_owned(),
    };
    std::fs::write(
        sidecar_dir.path().join("gateway-private.json"),
        serde_json::to_vec(&mismatched_private).expect("private json"),
    )
    .expect("write mismatched private");
    let mismatch_view = status_view_for(&ChatgptAccount::new(sidecar_dir.path())).await;
    assert_eq!(mismatch_view.state, "unreadable");
    assert!(
        mismatch_view.detail.contains("(pid, epoch) (7, 9)")
            && mismatch_view.detail.contains("(11, 13)"),
        "{}",
        mismatch_view.detail
    );
    assert!(
        !mismatch_view.detail.contains("boot_id"),
        "{}",
        mismatch_view.detail
    );
    assert_no_secret_values(&ipc_json(&mismatch_view), &forbidden);

    // Wrong-typed discovery file also keeps values and paths out of IPC.
    std::fs::write(
        sidecar_dir.path().join("gateway.json"),
        format!(r#"{{"port":"{BEARER_SECRET}"}}"#),
    )
    .expect("write wrong-typed discovery");
    let discovery_parse_view = status_view_for(&ChatgptAccount::new(sidecar_dir.path())).await;
    assert_eq!(discovery_parse_view.state, "unavailable");
    let discovery_json = ipc_json(&discovery_parse_view);
    assert_no_secret_values(&discovery_json, &forbidden);
    assert!(
        !discovery_json.contains(&dir_str),
        "run path leaked: {discovery_json}"
    );
}

#[tokio::test]
async fn malformed_sign_in_replies_and_url_echoing_openers_never_leak_oauth_secrets() {
    const STATE_RAW: &str = "synth-oauth-state-3a2b1c";
    const PKCE_RAW: &str = "synth-pkce-challenge-0z9y8x";
    const AUTHORIZE_URL: &str = "https://auth.example/oauth/authorize?state=synth-oauth-state-3a2b1c%2Btail&code_challenge=synth-pkce-challenge-0z9y8x%2Fpad";
    const PERCENT_ENCODED_URL: &str = "https%3A%2F%2Fauth.example%2Foauth%2Fauthorize%3Fstate%3Dsynth-oauth-state-3a2b1c%252Btail%26code_challenge%3Dsynth-pkce-challenge-0z9y8x%252Fpad";

    let forbidden = [
        "auth.example",
        STATE_RAW,
        PKCE_RAW,
        "%2Btail",
        "+tail",
        "%252Btail",
        "%2Fpad",
        "/pad",
        "%252Fpad",
        "opener-whole-echo",
        "opener-partial-echo",
        "opener-encoded-echo",
    ];

    // 3a. Truncated/malformed sign-in body carrying raw OAuth state & PKCE challenge.
    let truncated_body = format!(r#"{{"authorize_url":"{AUTHORIZE_URL}"#);
    let (malformed_dir, malformed_server) = spawn_fake_gateway(200, &truncated_body);
    let malformed_view = sign_in_view_for(&ChatgptAccount::new(malformed_dir.path()), |_| {
        Ok::<(), &'static str>(())
    })
    .await;
    let _ = malformed_server.join();
    assert_eq!(malformed_view.state, "unavailable");
    assert_no_secret_values(&ipc_json(&malformed_view), &forbidden);

    // 3b. Wrong-typed authorize_url field carrying OAuth secrets.
    let wrong_type_body = format!(r#"{{"authorize_url":["{STATE_RAW}","{PKCE_RAW}"]}}"#);
    let (wrong_dir, wrong_server) = spawn_fake_gateway(200, &wrong_type_body);
    let wrong_view = sign_in_view_for(&ChatgptAccount::new(wrong_dir.path()), |_| {
        Ok::<(), &'static str>(())
    })
    .await;
    let _ = wrong_server.join();
    assert_eq!(wrong_view.state, "unavailable");
    assert_no_secret_values(&ipc_json(&wrong_view), &forbidden);

    // 4. Failing opener echoing the URL whole, in part (decoded query values), and percent-encoded.
    let valid_body = format!(r#"{{"authorize_url":"{AUTHORIZE_URL}"}}"#);
    let opener_errors = [
        format!("opener-whole-echo: {AUTHORIZE_URL}"),
        format!("opener-partial-echo: state={STATE_RAW}+tail challenge={PKCE_RAW}/pad"),
        format!("opener-encoded-echo: {PERCENT_ENCODED_URL}"),
    ];
    for echoed in opener_errors {
        let (dir, server) = spawn_fake_gateway(200, &valid_body);
        let view =
            sign_in_view_for(&ChatgptAccount::new(dir.path()), |_url| Err(echoed.clone())).await;
        let _ = server.join();
        assert_eq!(view.state, "unavailable");
        assert_no_secret_values(&ipc_json(&view), &forbidden);
    }
}

#[test]
fn client_build_error_projects_to_unavailable_with_closed_category() {
    use workshop_server_api::{ChatgptAccountError, ChatgptHttpFailure};

    let view = super::view_of_account_error(ChatgptAccountError::ClientBuild(
        ChatgptHttpFailure::Builder,
    ));
    assert_eq!(view.state, "unavailable");
    assert_eq!(
        view.detail,
        "Loopback HTTP account client failed to build: client builder error"
    );
    assert!(view.email.is_none());
    assert!(!view.plan_granted);
    assert!(view.plan_models.is_empty());
    assert_no_secret_values(&ipc_json(&view), &[]);
}
