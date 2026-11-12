//! `cargo workshop bundle`: builds Workshop for one target in release and
//! bundles it with the pinned Tauri CLI. The output is Tauri's bundle
//! directory for the platform, `<triple>/release/bundle/<nsis|macos|appimage>`
//! in the target directory: the NSIS setup on Windows, `Runvil.app` on
//! macOS, and the AppImage on Linux, each with Workshop's updater file and
//! its signature under `--sign`. The Gateway is the release
//! `gateway-pin/pin.json` names, staged by `tools/fetch-gateway-sidecar.mjs`
//! or named by `--gateway-binary`. Only the Windows setup holds the
//! Gateway, as Workshop's sidecar. The installers are built from this
//! bundle in `cppalliance/runvil`.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::args::BundleRequest;
use super::pipeline::{resolve_target, run_checked, with_staged_sidecar};
use super::sidecar::{self, GatewaySource};
use super::{
    BuildEnvironment, BuildError, CommandRunner, CommandSpec, OutputMode, StepError, plugin,
};

/// Applied to unsigned builds: it turns off `createUpdaterArtifacts`, which
/// would otherwise demand the release key.
const UNSIGNED_CONFIG: &str = "tauri.unsigned.conf.json";
/// The Tauri CLI first runs after the Gateway version check, so a missing
/// `node` is found up front instead.
const NODE_CHECK: &str = "`node --version` (the Tauri CLI runs on `node` from PATH)";

/// The Gateway release a bundle is built with: its executable and the
/// version `gateway-pin/pin.json` names.
#[derive(Debug, Eq, PartialEq)]
struct Gateway {
    binary: PathBuf,
    version: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum System {
    Windows,
    MacOs,
    Linux,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Platform {
    pub(super) system: System,
}

impl Platform {
    pub(super) fn from_triple(target: &str) -> Result<Self, String> {
        let parts: Vec<&str> = target.split('-').collect();
        let supported_arch = matches!(parts.first(), Some(&("x86_64" | "aarch64")));
        let system = match parts.get(1..) {
            Some(["pc", "windows", "msvc"]) => Some(System::Windows),
            Some(["apple", "darwin"]) => Some(System::MacOs),
            Some(["unknown", "linux", "gnu"]) => Some(System::Linux),
            _ => None,
        };
        match system {
            Some(system) if supported_arch => Ok(Self { system }),
            _ => Err(format!(
                "bundle target `{target}` is not supported; supported: x86_64 or aarch64 \
                 with pc-windows-msvc, apple-darwin, or unknown-linux-gnu"
            )),
        }
    }

    /// The Tauri bundle format, which is also its output directory's name
    /// except for `app`.
    pub(super) fn bundle(self) -> &'static str {
        match self.system {
            System::Windows => "nsis",
            System::MacOs => "app",
            System::Linux => "appimage",
        }
    }

    fn bundle_directory(self) -> &'static str {
        match self.system {
            System::MacOs => "macos",
            System::Windows | System::Linux => self.bundle(),
        }
    }
}

pub(super) fn build_bundle(
    request: &BundleRequest,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<(), BuildError> {
    preflight(request, environment)?;
    let target = resolve_target(request.target.as_deref(), environment, runner)?;
    let platform = Platform::from_triple(&target).map_err(failure)?;
    let given = request
        .gateway_binary
        .as_deref()
        .or(environment.gateway_sidecar.as_deref());
    let source =
        sidecar::gateway_source(given, &environment.workspace_root, &target).map_err(failure)?;
    if source == (GatewaySource::Staged { stub: true }) {
        return Err(failure(format!(
            "the Gateway staged for {target} at {} is a compile-only stub, which a bundle \
             cannot ship; run `node tools/fetch-gateway-sidecar.mjs --target {target}` to stage \
             the release gateway-pin/pin.json names, or pass --gateway-binary",
            sidecar::sidecar_path(&environment.workspace_root, &target).display()
        )));
    }
    let gateway = Gateway {
        binary: source.binary(&environment.workspace_root, &target),
        version: sidecar::pinned_version(&environment.workspace_root).map_err(failure)?,
    };
    run_checked(runner, &node_check_command(environment), NODE_CHECK)?;
    let release = environment.target_root.join(&target).join("release");
    let bundle_directory = release.join("bundle").join(platform.bundle_directory());
    // A bundle left by an earlier build, possibly at another version, must
    // not sit beside this build's in the directory this mode prints.
    remove_directory(&bundle_directory)?;
    check_gateway_version(runner, environment, &gateway)?;
    // The Windows setup takes the Plugins' libraries from the build
    // itself. On macOS and Linux Tauri copies them into Workshop's bundle,
    // so the updater replaces them with Workshop.
    let libraries = plugin::built_libraries(&release, &target);
    let plugin_config = plugin::bundle_config(platform.system, &libraries).map_err(failure)?;
    with_staged_sidecar(
        environment,
        &target,
        runner,
        "Workshop bundle",
        &source,
        |runner| {
            let mut bundle = tauri_build_command(
                environment,
                &target,
                platform,
                request.sign,
                plugin_config.as_deref(),
            );
            bundle.envs.extend(source.child_envs());
            run_checked(runner, &bundle, "Workshop bundle").map(|_| ())
        },
    )?;
    plugin::check_bundled(
        platform.system,
        &bundle_directory,
        &plugin::library_names(&target),
    )
    .map_err(failure)?;
    println!("bundle: {}", bundle_directory.display());
    Ok(())
}

fn preflight(request: &BundleRequest, environment: &BuildEnvironment) -> Result<(), BuildError> {
    if !environment.tauri_cli.is_file() {
        return Err(failure(format!(
            "the Tauri CLI is missing at {}; run `npm ci --prefix crates/workshop` first",
            environment.tauri_cli.display()
        )));
    }
    if request.sign && environment.signing_key.is_none() {
        return Err(failure(
            "`--sign` needs TAURI_SIGNING_PRIVATE_KEY set to the release minisign key; it is \
             unset or empty"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Runs the Gateway's `--version` and requires the version the pin names,
/// so no bundle is built with a Gateway other than the pinned release.
fn check_gateway_version(
    runner: &mut impl CommandRunner,
    environment: &BuildEnvironment,
    gateway: &Gateway,
) -> Result<(), StepError> {
    let printed = run_checked(
        runner,
        &CommandSpec {
            program: gateway.binary.clone(),
            args: vec![OsString::from("--version")],
            current_dir: environment.workspace_root.clone(),
            envs: Vec::new(),
            output_mode: OutputMode::Capture,
        },
        "Gateway version check",
    )?;
    let expected = format!("runvil-gateway {}", gateway.version);
    if printed.trim() == expected {
        Ok(())
    } else {
        Err(StepError::failed(format!(
            "Gateway version check failed: {} printed `{}`, expected `{expected}`, the \
             version gateway-pin/pin.json names",
            gateway.binary.display(),
            printed.trim()
        )))
    }
}

/// The Tauri build of Workshop's bundle for `target`. `plugin_config` is
/// the configuration that copies the Plugins' libraries into the bundle,
/// when the platform has one.
fn tauri_build_command(
    environment: &BuildEnvironment,
    target: &str,
    platform: Platform,
    sign: bool,
    plugin_config: Option<&str>,
) -> CommandSpec {
    let mut args = vec![environment.tauri_cli.clone().into_os_string()];
    args.extend(
        [
            "build",
            "--ci",
            "--target",
            target,
            "--bundles",
            platform.bundle(),
        ]
        .map(OsString::from),
    );
    if !sign {
        args.extend(["--config", UNSIGNED_CONFIG].map(OsString::from));
    }
    if let Some(config) = plugin_config {
        args.extend(["--config", config].map(OsString::from));
    }
    CommandSpec {
        program: environment.node.clone(),
        args,
        current_dir: environment
            .workspace_root
            .join("crates")
            .join("workshop")
            .join("desktop"),
        envs: environment.cargo_envs(),
        output_mode: OutputMode::Inherit,
    }
}

fn node_check_command(environment: &BuildEnvironment) -> CommandSpec {
    CommandSpec {
        program: environment.node.clone(),
        args: vec![OsString::from("--version")],
        current_dir: environment.workspace_root.clone(),
        envs: Vec::new(),
        output_mode: OutputMode::Capture,
    }
}

fn remove_directory(path: &Path) -> Result<(), BuildError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failure(format!("cannot clear {}: {error}", path.display()))),
    }
}

fn failure(message: String) -> BuildError {
    BuildError {
        primary: message,
        cleanup: None,
    }
}
