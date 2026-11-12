//! Unit tests for the build and bundle modes' arguments.

use super::*;

pub(super) fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn parses_only_the_documented_profile_and_target_options() {
    assert_eq!(
        parse_arguments(&[]).expect("debug request"),
        Request::Build(BuildRequest {
            profile: Profile::Debug,
            target: None,
            gateway_binary: None,
        })
    );
    assert_eq!(
        parse_arguments(&arguments(&[
            "--target",
            "aarch64-apple-darwin",
            "--release",
            "--gateway-binary",
            "gateway/runvil-gateway",
        ]))
        .expect("release target request"),
        Request::Build(BuildRequest {
            profile: Profile::Release,
            target: Some("aarch64-apple-darwin".to_owned()),
            gateway_binary: Some(PathBuf::from("gateway/runvil-gateway")),
        })
    );
}

#[test]
fn parses_the_bundle_mode_with_its_gateway_binary() {
    assert_eq!(
        parse_arguments(&arguments(&["bundle"])).expect("bundle"),
        Request::Bundle(BundleRequest {
            target: None,
            sign: false,
            gateway_binary: None,
        })
    );
    assert_eq!(
        parse_arguments(&arguments(&[
            "bundle",
            "--gateway-binary",
            "gateway/runvil-gateway.exe",
            "--sign",
            "--target",
            "x86_64-pc-windows-msvc",
        ]))
        .expect("signed bundle"),
        Request::Bundle(BundleRequest {
            target: Some("x86_64-pc-windows-msvc".to_owned()),
            sign: true,
            gateway_binary: Some(PathBuf::from("gateway/runvil-gateway.exe")),
        })
    );
}

#[test]
fn rejects_product_features_and_other_unsupported_arguments() {
    for args in [
        arguments(&["--features", "local"]),
        arguments(&["--profile", "dist"]),
        arguments(&["gateway"]),
        arguments(&["installer"]),
        arguments(&["manifest"]),
        arguments(&["--sign"]),
        arguments(&["bundle", "--source", "gateway"]),
        // The sidecar mode is gone; the slot is the fetch script's.
        arguments(&["sidecar", "stage", "--target", "x86_64-pc-windows-msvc"]),
    ] {
        let error = parse_arguments(&args)
            .expect_err("unsupported argument")
            .to_string();
        assert!(error.contains("unsupported argument"), "{args:?}: {error}");
        assert!(error.contains(args::USAGE), "{error}");
    }
}

#[test]
fn rejects_duplicate_incomplete_or_malformed_options() {
    for args in [
        arguments(&["--release", "--release"]),
        arguments(&["--target"]),
        arguments(&[
            "--target",
            "x86_64-pc-windows-msvc",
            "--target",
            "x86_64-unknown-linux-gnu",
        ]),
        arguments(&["--target", "../outside"]),
        arguments(&["--gateway-binary"]),
        arguments(&["--gateway-binary", "a", "--gateway-binary", "b"]),
        arguments(&["bundle", "--sign", "--sign"]),
        arguments(&["bundle", "--target", "--sign"]),
        arguments(&["bundle", "--gateway-binary", "--sign"]),
    ] {
        assert!(parse_arguments(&args).is_err(), "{args:?}");
    }
}

#[test]
fn bundle_release_says_the_bundle_always_builds_release() {
    let error = parse_arguments(&arguments(&["bundle", "--target", "x", "--release"]))
        .expect_err("bundle --release")
        .to_string();

    assert!(
        error.starts_with("`bundle` always builds the release profile; drop `--release`"),
        "{error}"
    );
}
