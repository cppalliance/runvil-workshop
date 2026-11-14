use std::io::{Read as _, Write as _};

use workshop_gateway_discovery::{GatewayDiscoveryFile, GatewayPrivateFile, SidecarError};

use super::{
    BootSecrets, ChatgptAccount, ChatgptAccountError, ChatgptAccountOp, ChatgptJsonFailure,
    ChatgptReadTarget, ChatgptSidecarFailure, authorize_url_from_body,
};

const fn assert_copy<T: Copy>() {}

#[test]
fn account_error_types_are_copy_and_boot_secrets_redact_debug() {
    assert_copy::<ChatgptAccountError>();
    assert_copy::<ChatgptAccountOp>();
    assert_copy::<ChatgptReadTarget>();
    assert_copy::<ChatgptSidecarFailure>();
    assert_copy::<ChatgptJsonFailure>();
    assert_copy::<super::ChatgptHttpFailure>();
    assert_eq!(
        ChatgptSidecarFailure::from_sidecar(&SidecarError::Canceled),
        ChatgptSidecarFailure::Other
    );

    let secrets = BootSecrets {
        base_url: "http://127.0.0.1:41234".to_owned(),
        api_key: "secret-api-bearer".to_owned(),
        owner_proof: "secret-owner-proof".to_owned(),
    };
    let formatted = format!("{secrets:?}");
    assert!(formatted.contains("127.0.0.1:41234"), "{formatted}");
    assert!(formatted.contains("<redacted>"), "{formatted}");
    assert!(!formatted.contains("secret-api-bearer"), "{formatted}");
    assert!(!formatted.contains("secret-owner-proof"), "{formatted}");
}

#[test]
fn authorize_url_parser_extracts_non_empty_url_or_returns_closed_error() {
    let url =
        authorize_url_from_body(r#"{"authorize_url":"https://auth.example/go"}"#).expect("url");
    assert_eq!(url, "https://auth.example/go");

    assert_eq!(
        authorize_url_from_body(r#"{"authorize_url":""}"#),
        Err(ChatgptAccountError::SignInMissingUrl)
    );
    assert!(matches!(
        authorize_url_from_body(r#"{"authorize_url":"https://auth.example/unterminated"#),
        Err(ChatgptAccountError::SignInInvalidJson(
            ChatgptJsonFailure::Eof { .. }
        ))
    ));
}

#[tokio::test]
async fn boot_secrets_reject_missing_and_mismatched_sidecars() {
    let empty = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        ChatgptAccount::new(empty.path()).status().await,
        Err(ChatgptAccountError::DiscoveryMissing)
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let discovery = GatewayDiscoveryFile {
        port: 41235,
        api_key: "loopback-bearer-secret".to_owned(),
        pid: 42,
        epoch: 99,
        version: "test".to_owned(),
        started_at: "1970-01-01T00:00:09Z".to_owned(),
    };
    std::fs::write(
        dir.path().join("gateway.json"),
        serde_json::to_vec(&discovery).expect("discovery json"),
    )
    .expect("write discovery");

    assert_eq!(
        ChatgptAccount::new(dir.path()).status().await,
        Err(ChatgptAccountError::OwnerProofMissing)
    );

    let stale_private = GatewayPrivateFile {
        pid: 42,
        epoch: 100,
        plan_proof: "plan-proof".to_owned(),
        owner_proof: "owner-proof-secret".to_owned(),
    };
    std::fs::write(
        dir.path().join("gateway-private.json"),
        serde_json::to_vec(&stale_private).expect("stale private json"),
    )
    .expect("write stale private");
    assert_eq!(
        ChatgptAccount::new(dir.path()).status().await,
        Err(ChatgptAccountError::BootMismatch {
            discovery_pid: 42,
            discovery_epoch: 99,
            private_pid: 42,
            private_epoch: 100,
        })
    );
}

#[tokio::test]
async fn loopback_requests_attach_owner_proof_and_refuse_redirects() {
    let target_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind redirect canary");
    target_listener
        .set_nonblocking(true)
        .expect("nonblocking canary");
    let target_port = target_listener.local_addr().expect("canary addr").port();

    let gateway_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
    let gateway_port = gateway_listener.local_addr().expect("gateway addr").port();
    let server = std::thread::spawn(move || {
        let (mut sock, _) = gateway_listener.accept().expect("accept request");
        let mut buf = [0_u8; 4096];
        let mut filled = 0;
        while filled < buf.len() {
            let read = sock.read(&mut buf[filled..]).unwrap_or(0);
            if read == 0 {
                break;
            }
            filled += read;
            if buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let req_text = String::from_utf8_lossy(&buf[..filled]).into_owned();
        let reply = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = sock.write_all(reply.as_bytes());
        req_text
    });

    let dir = tempfile::tempdir().expect("tempdir");
    let discovery = GatewayDiscoveryFile {
        port: gateway_port,
        api_key: "loopback-bearer-secret".to_owned(),
        pid: 42,
        epoch: 99,
        version: "test".to_owned(),
        started_at: "1970-01-01T00:00:09Z".to_owned(),
    };
    let matching_private = GatewayPrivateFile {
        pid: 42,
        epoch: 99,
        plan_proof: "plan-proof".to_owned(),
        owner_proof: "owner-proof-secret".to_owned(),
    };
    std::fs::write(
        dir.path().join("gateway.json"),
        serde_json::to_vec(&discovery).expect("discovery json"),
    )
    .expect("write discovery");
    std::fs::write(
        dir.path().join("gateway-private.json"),
        serde_json::to_vec(&matching_private).expect("matching private json"),
    )
    .expect("write matching private");

    let status_result = ChatgptAccount::new(dir.path()).status().await;
    assert_eq!(
        status_result,
        Err(ChatgptAccountError::HttpStatus {
            op: ChatgptAccountOp::Status,
            status: 302,
        })
    );
    let captured_headers = server
        .join()
        .expect("join fake gateway")
        .to_ascii_lowercase();
    assert!(
        captured_headers.contains("authorization: bearer loopback-bearer-secret"),
        "{captured_headers}"
    );
    assert!(
        captured_headers.contains("x-runvil-owner-proof: owner-proof-secret"),
        "{captured_headers}"
    );
    assert!(
        target_listener.accept().is_err(),
        "redirect target must never be contacted with Gateway secrets"
    );
}
