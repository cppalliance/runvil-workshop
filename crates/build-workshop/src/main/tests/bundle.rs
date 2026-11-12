//! Unit tests for the bundle mode's platform table, its command sequence,
//! and its preflight.

use super::*;
use crate::bundle::{Platform, System};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn release(environment: &BuildEnvironment, target: &str) -> PathBuf {
    environment.target_root.join(target).join("release")
}

/// The staged Gateway, which the version check runs.
fn gateway(environment: &BuildEnvironment, target: &str) -> PathBuf {
    sidecar::sidecar_path(&environment.workspace_root, target)
}

/// Tauri's bundle directory named `directory` for `target`, which the
/// bundle mode prints.
fn bundle_directory(environment: &BuildEnvironment, target: &str, directory: &str) -> PathBuf {
    release(environment, target).join("bundle").join(directory)
}

/// The signature Tauri writes beside the updater file `path` under
/// `--sign`.
fn signature(path: &Path) -> PathBuf {
    let mut signature = path.as_os_str().to_owned();
    signature.push(".sig");
    PathBuf::from(signature)
}

pub(super) fn node_found() -> FakeResponse {
    success("v22.0.0\n")
}

pub(super) fn version_printed() -> FakeResponse {
    success(&format!("runvil-gateway {GATEWAY_VERSION}\n"))
}

/// A Tauri build that writes `files`, with a Gateway in the slot: one the
/// fetch script staged for any target, or a given one staged for Windows.
pub(super) fn bundled(
    sidecar: PathBuf,
    files: Vec<PathBuf>,
    response: FakeResponse,
) -> FakeResponse {
    act(
        move || {
            assert!(
                sidecar.is_file(),
                "no Gateway in the slot during the bundle"
            );
            for file in files {
                write_file(&file, b"bundle");
            }
        },
        response,
    )
}

/// The library of each Plugin Workshop ships, inside the Workshop bundle
/// a Tauri build leaves for a macOS or Linux `target`, where the build's
/// configuration has Tauri copy them.
pub(super) fn bundled_plugins(environment: &BuildEnvironment, target: &str) -> Vec<PathBuf> {
    let (plugins, names) = if target.contains("apple") {
        (
            bundle_directory(environment, target, "macos")
                .join("Runvil.app")
                .join("Contents")
                .join("Resources")
                .join("plugins"),
            ["libplugin_std.dylib", "libplugin_bashkit.dylib"],
        )
    } else {
        (
            bundle_directory(environment, target, "appimage")
                .join("Runvil.AppDir")
                .join("usr")
                .join("lib")
                .join("runvil")
                .join("plugins"),
            ["libplugin_std.so", "libplugin_bashkit.so"],
        )
    };
    names.iter().map(|name| plugins.join(name)).collect()
}

/// The `--config` text that has Tauri copy the libraries of `target`'s
/// release build into Workshop's bundle, or `None` for Windows.
fn plugin_config(environment: &BuildEnvironment, target: &str) -> Option<String> {
    let deps = release(environment, target).join("deps");
    let config = if target.contains("apple") {
        serde_json::json!({
            "bundle": { "macOS": { "files": {
                "Resources/plugins/libplugin_std.dylib": deps.join("libplugin_std.dylib"),
                "Resources/plugins/libplugin_bashkit.dylib":
                    deps.join("libplugin_bashkit.dylib"),
            } } }
        })
    } else if target.contains("linux") {
        serde_json::json!({
            "bundle": { "linux": { "appimage": { "files": {
                "/usr/lib/runvil/plugins/libplugin_std.so": deps.join("libplugin_std.so"),
                "/usr/lib/runvil/plugins/libplugin_bashkit.so":
                    deps.join("libplugin_bashkit.so"),
            } } } }
        })
    } else {
        return None;
    };
    Some(config.to_string())
}

/// The commands a bundle build of `target` runs once the target is known:
/// the `node` check, the Gateway version check, and the Tauri build of
/// the bundle format `format`.
fn expected_commands(
    environment: &BuildEnvironment,
    target: &str,
    format: &str,
    sign: bool,
) -> Vec<CommandSpec> {
    let plugin_config = plugin_config(environment, target);
    let mut tauri = vec![
        environment.tauri_cli.to_str().expect("UTF-8 CLI"),
        "build",
        "--ci",
        "--target",
        target,
        "--bundles",
        format,
    ];
    if !sign {
        tauri.extend(["--config", "tauri.unsigned.conf.json"]);
    }
    // The libraries of this release build go into the bundle, so the
    // updater replaces them with Workshop.
    if let Some(config) = &plugin_config {
        tauri.extend(["--config", config.as_str()]);
    }
    vec![
        command(
            environment,
            "selected-node",
            &["--version"],
            OutputMode::Capture,
        ),
        CommandSpec {
            program: gateway(environment, target),
            args: strings(&["--version"]),
            current_dir: environment.workspace_root.clone(),
            envs: Vec::new(),
            output_mode: OutputMode::Capture,
        },
        CommandSpec {
            current_dir: environment
                .workspace_root
                .join("crates")
                .join("workshop")
                .join("desktop"),
            envs: target_root_env(environment),
            ..command(environment, "selected-node", &tauri, OutputMode::Inherit)
        },
    ]
}

pub(super) fn request(target: &str, sign: bool) -> BundleRequest {
    BundleRequest {
        target: Some(target.to_owned()),
        sign,
        gateway_binary: None,
    }
}

#[test]
fn maps_each_supported_triple_to_its_platform() {
    for (target, system) in [
        ("x86_64-pc-windows-msvc", System::Windows),
        ("aarch64-pc-windows-msvc", System::Windows),
        ("aarch64-apple-darwin", System::MacOs),
        ("x86_64-apple-darwin", System::MacOs),
        ("x86_64-unknown-linux-gnu", System::Linux),
        ("aarch64-unknown-linux-gnu", System::Linux),
    ] {
        assert_eq!(
            Platform::from_triple(target),
            Ok(Platform { system }),
            "{target}"
        );
    }
    for target in [
        "x86_64-pc-windows-gnu",
        "x86_64-unknown-linux-musl",
        "i686-pc-windows-msvc",
        "x86_64-unknown-freebsd",
        "x86_64-uwp-windows-msvc",
        "aarch64-apple-ios",
        "x86_64-unknown-linux-gnux32",
    ] {
        let error = Platform::from_triple(target).expect_err(target);
        assert!(error.contains(target), "{error}");
    }
}

#[test]
fn windows_unsigned_builds_the_setup_on_the_unsigned_config() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(target);
    let nsis = bundle_directory(environment, target, "nsis");
    let setup = nsis.join(format!("Runvil_{VERSION}_x64-setup.exe"));
    // A setup left by an earlier build at another version must not stay
    // beside this build's.
    let stale = nsis.join("Runvil_0.0.1_x64-setup.exe");
    write_file(&stale, b"stale");
    let mut runner = FakeRunner::with_responses(vec![
        node_found(),
        version_printed(),
        bundled(staged.clone(), vec![setup.clone()], success("")),
    ]);

    build_bundle(&request(target, false), environment, &mut runner).expect("bundle");

    assert_eq!(
        runner.commands,
        expected_commands(environment, target, "nsis", false)
    );
    assert!(setup.is_file());
    assert!(!stale.exists(), "an earlier build's setup was left");
    assert_eq!(std::fs::read_dir(&nsis).expect("bundle").count(), 1);
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn windows_signed_leaves_the_setup_and_its_signature_in_the_bundle_directory() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(target);
    let nsis = bundle_directory(environment, target, "nsis");
    let setup = nsis.join(format!("Runvil_{VERSION}_x64-setup.exe"));
    let mut runner = FakeRunner::with_responses(vec![
        node_found(),
        version_printed(),
        bundled(staged, vec![signature(&setup), setup.clone()], success("")),
    ]);

    build_bundle(&request(target, true), environment, &mut runner).expect("bundle");

    assert_eq!(
        runner.commands,
        expected_commands(environment, target, "nsis", true)
    );
    assert!(setup.is_file());
    assert!(signature(&setup).is_file());
    assert_eq!(std::fs::read_dir(&nsis).expect("bundle").count(), 2);
}

#[test]
fn linux_unsigned_from_the_host_bundles_the_appimage_with_the_plugin_configuration() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    let staged = test_environment.stage_gateway(target);
    let directory = bundle_directory(environment, target, "appimage");
    let appimage = directory.join(format!("Runvil_{VERSION}_amd64.AppImage"));
    let stale = directory.join("Runvil_0.0.1_amd64.AppImage");
    write_file(&stale, b"stale");
    let mut runner = FakeRunner::with_responses(vec![
        success(&format!("cargo 1.89.0\nhost: {target}\n")),
        node_found(),
        version_printed(),
        bundled(
            staged.clone(),
            [
                vec![
                    appimage.clone(),
                    directory.join("Runvil.AppDir").join("AppRun"),
                ],
                bundled_plugins(environment, target),
            ]
            .concat(),
            success(""),
        ),
    ]);

    build_bundle(
        &BundleRequest {
            target: None,
            sign: false,
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect("bundle");

    let mut expected = vec![command(
        environment,
        "selected-cargo",
        &["-vV"],
        OutputMode::Capture,
    )];
    expected.extend(expected_commands(environment, target, "appimage", false));
    assert_eq!(runner.commands, expected);
    assert!(appimage.is_file());
    assert!(!stale.exists(), "an earlier build's AppImage was left");
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn linux_signed_leaves_the_appimage_and_its_signature_in_the_bundle_directory() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "aarch64-unknown-linux-gnu";
    let staged = test_environment.stage_gateway(target);
    let appimage = bundle_directory(environment, target, "appimage")
        .join(format!("Runvil_{VERSION}_aarch64.AppImage"));
    let mut runner = FakeRunner::with_responses(vec![
        node_found(),
        version_printed(),
        bundled(
            staged,
            [
                vec![signature(&appimage), appimage.clone()],
                bundled_plugins(environment, target),
            ]
            .concat(),
            success(""),
        ),
    ]);

    build_bundle(&request(target, true), environment, &mut runner).expect("bundle");

    assert_eq!(
        runner.commands,
        expected_commands(environment, target, "appimage", true)
    );
    assert!(appimage.is_file());
    assert!(signature(&appimage).is_file());
}

#[test]
fn macos_unsigned_bundles_the_app_with_the_plugin_configuration() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-apple-darwin";
    let staged = test_environment.stage_gateway(target);
    let directory = bundle_directory(environment, target, "macos");
    let workshop = directory
        .join("Runvil.app")
        .join("Contents")
        .join("MacOS")
        .join("runvil-workshop");
    let stale = directory.join("Runvil.app.tar.gz");
    write_file(&stale, b"stale");
    let mut runner = FakeRunner::with_responses(vec![
        node_found(),
        version_printed(),
        bundled(
            staged.clone(),
            [vec![workshop.clone()], bundled_plugins(environment, target)].concat(),
            success(""),
        ),
    ]);

    build_bundle(&request(target, false), environment, &mut runner).expect("bundle");

    assert_eq!(
        runner.commands,
        expected_commands(environment, target, "app", false)
    );
    assert!(workshop.is_file());
    assert!(!stale.exists(), "an earlier build's updater file was left");
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn macos_signed_leaves_the_app_and_its_signed_updater_file_in_the_bundle_directory() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "aarch64-apple-darwin";
    let staged = test_environment.stage_gateway(target);
    let directory = bundle_directory(environment, target, "macos");
    let workshop = directory
        .join("Runvil.app")
        .join("Contents")
        .join("MacOS")
        .join("runvil-workshop");
    let updater = directory.join("Runvil.app.tar.gz");
    let mut runner = FakeRunner::with_responses(vec![
        node_found(),
        version_printed(),
        bundled(
            staged,
            [
                vec![workshop.clone(), signature(&updater), updater.clone()],
                bundled_plugins(environment, target),
            ]
            .concat(),
            success(""),
        ),
    ]);

    build_bundle(&request(target, true), environment, &mut runner).expect("bundle");

    assert_eq!(
        runner.commands,
        expected_commands(environment, target, "app", true)
    );
    assert!(workshop.is_file());
    assert!(updater.is_file());
    assert!(signature(&updater).is_file());
}

#[test]
fn preflight_failures_run_no_command() {
    let mut test_environment = environment();
    let target = "x86_64-unknown-linux-gnu";
    test_environment.stage_gateway(target);
    let mut runner = FakeRunner::default();
    let mut refusal = |environment: &BuildEnvironment, case: &str| {
        build_bundle(&request(target, true), environment, &mut runner)
            .expect_err(case)
            .primary
    };

    let error = refusal(&test_environment.environment, "no signing key");
    assert!(error.contains("TAURI_SIGNING_PRIVATE_KEY set"), "{error}");

    set_signing_key(&mut test_environment.environment);
    std::fs::remove_file(&test_environment.environment.tauri_cli).expect("remove CLI");
    let error = refusal(&test_environment.environment, "no CLI");
    assert!(error.contains("npm ci --prefix crates/workshop"), "{error}");
    assert!(runner.commands.is_empty());
}
