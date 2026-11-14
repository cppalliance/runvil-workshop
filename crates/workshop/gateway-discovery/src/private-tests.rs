//! The private file pairs a plan proof to one boot, redacts both proofs,
//! and yields no proof when the pair does not hold.

use std::fs;

use super::{GatewayPrivateFile, paired_proof, plan_proof_for_boot};
use crate::file::GatewayDiscoveryFile;
use crate::paths::gateway_private_file_path;

const PLAN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OWNER: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn file() -> GatewayPrivateFile {
    GatewayPrivateFile {
        pid: 4242,
        epoch: 1_757_000_000,
        plan_proof: PLAN.to_owned(),
        owner_proof: OWNER.to_owned(),
    }
}

fn discovery(pid: u32, epoch: u64) -> GatewayDiscoveryFile {
    GatewayDiscoveryFile {
        port: 8081,
        api_key: "key".to_owned(),
        pid,
        epoch,
        version: "0.3.0".to_owned(),
        started_at: "2026-11-13T12:00:00Z".to_owned(),
    }
}

fn write(dir: &std::path::Path, value: &impl serde::Serialize) {
    fs::write(
        gateway_private_file_path(dir),
        serde_json::to_vec(value).expect("the fixture serializes"),
    )
    .expect("the fixture writes");
}

#[test]
fn a_matching_file_returns_the_plan_proof() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    write(dir.path(), &file());
    assert_eq!(
        plan_proof_for_boot(dir.path(), 4242, 1_757_000_000).as_deref(),
        Some(PLAN)
    );
}

#[test]
fn a_missing_file_returns_none() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    assert_eq!(plan_proof_for_boot(dir.path(), 4242, 1_757_000_000), None);
    assert_eq!(
        GatewayPrivateFile::read(dir.path()).expect("a missing file is not an error"),
        None
    );
}

#[test]
fn a_private_file_for_another_boot_returns_none() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    write(dir.path(), &file());
    fs::write(
        dir.path().join("gateway.json"),
        serde_json::to_vec(&discovery(9999, 2)).expect("discovery serializes"),
    )
    .expect("discovery writes");
    assert_eq!(
        plan_proof_for_boot(dir.path(), 9999, 2),
        None,
        "a proof from another boot is not used"
    );
    assert_eq!(
        plan_proof_for_boot(dir.path(), 4242, 1).as_deref(),
        None,
        "a different epoch is another boot"
    );
}

#[test]
fn a_second_read_uses_the_proof_only_when_both_files_name_the_requested_boot() {
    let private = file();
    let same = discovery(4242, 1_757_000_000);
    assert_eq!(
        paired_proof(&private, &same, 4242, 1_757_000_000).as_deref(),
        Some(PLAN)
    );
    let other = discovery(9999, 2);
    assert_eq!(paired_proof(&private, &other, 4242, 1_757_000_000), None);
    assert_eq!(paired_proof(&private, &same, 9999, 2), None);
}

#[test]
fn an_invalid_file_is_an_error_whose_text_omits_both_proofs() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let mut invalid = file();
    invalid.pid = 0;
    write(dir.path(), &invalid);
    let error = GatewayPrivateFile::read(dir.path()).expect_err("pid 0 is invalid");
    let rendered = error.to_string();
    assert!(rendered.contains("pid must not be 0"), "{rendered}");
    assert!(!rendered.contains(PLAN), "{rendered}");
    assert!(!rendered.contains(OWNER), "{rendered}");
    assert_eq!(plan_proof_for_boot(dir.path(), 0, 1_757_000_000), None);
}

#[test]
fn validation_refuses_an_empty_proof_a_space_and_two_equal_proofs() {
    let mut empty = file();
    empty.plan_proof.clear();
    assert_eq!(empty.validation_error(), Some("a proof must not be empty"));
    let mut spaced = file();
    spaced.plan_proof = "not a header".to_owned();
    assert_eq!(
        spaced.validation_error(),
        Some("a proof must be printable ASCII with no spaces")
    );
    let mut same = file();
    same.owner_proof = same.plan_proof.clone();
    assert_eq!(same.validation_error(), Some("the two proofs must differ"));
}

#[test]
fn an_unknown_field_is_tolerated() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let raw = serde_json::json!({
        "pid": 4242,
        "epoch": 1_757_000_000,
        "plan_proof": PLAN,
        "owner_proof": OWNER,
        "extra": "kept-by-a-newer-gateway",
    });
    write(dir.path(), &raw);
    let read = GatewayPrivateFile::read(dir.path())
        .expect("unknown fields are not an error")
        .expect("the file exists");
    assert_eq!(read, file());
}

#[test]
fn debug_redacts_both_proofs() {
    let rendered = format!("{:?}", file());
    assert!(!rendered.contains(PLAN), "{rendered}");
    assert!(!rendered.contains(OWNER), "{rendered}");
    assert!(rendered.contains("[REDACTED]"), "{rendered}");
    assert!(rendered.contains("4242"), "{rendered}");
}
