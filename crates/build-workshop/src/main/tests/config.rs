//! Unit tests for the per-platform Tauri configs.

use serde_json::Value;

use super::*;
use crate::bundle::Platform;

fn tauri_config(name: &str) -> Value {
    let path = repository_root()
        .join("crates")
        .join("workshop")
        .join("desktop")
        .join(name);
    let text = std::fs::read_to_string(&path).expect("Tauri config");
    serde_json::from_str(&text).expect("Tauri config JSON")
}

#[test]
fn each_platform_config_builds_the_installer_bundle_and_only_windows_carries_the_gateway() {
    let base = tauri_config("tauri.conf.json");
    assert_eq!(base["bundle"].get("targets"), None);
    assert_eq!(base["bundle"].get("externalBin"), None);
    for (file, target) in [
        ("tauri.windows.conf.json", "x86_64-pc-windows-msvc"),
        ("tauri.macos.conf.json", "aarch64-apple-darwin"),
        ("tauri.linux.conf.json", "x86_64-unknown-linux-gnu"),
    ] {
        let config = tauri_config(file);
        let platform = Platform::from_triple(target).expect("platform");
        assert_eq!(
            config["bundle"]["targets"],
            serde_json::json!([platform.bundle()]),
            "{file}"
        );
        let external = config["bundle"].get("externalBin");
        if sidecar::bundles_sidecar(target) {
            assert_eq!(
                external,
                Some(&serde_json::json!(["binaries/runvil-gateway"])),
                "{file}"
            );
        } else {
            assert_eq!(external, None, "{file}");
        }
    }
}

#[test]
fn the_appimage_bundles_the_gstreamer_plugins_beside_the_core_webkit_brings() {
    let linux = tauri_config("tauri.linux.conf.json");
    assert_eq!(
        linux["bundle"]["linux"]["appimage"]["bundleMediaFramework"],
        Value::Bool(true)
    );
}
