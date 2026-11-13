# build-workshop

This crate is `cargo workshop`, the one build path for Workshop. It builds Workshop for development, and it builds the Tauri bundle that each platform's installer is made from. The installers are built from that bundle in [cppalliance/runvil](https://github.com/cppalliance/runvil), which holds the Qt IFW definition, the install tests, and the release workflow.

## Modes

- `cargo workshop [--release] [--target <triple>] [--gateway-binary <path>]` builds Workshop with Cargo. For a Windows target the Gateway is Workshop's sidecar; elsewhere the build copies it beside Workshop in the target directory, where a development Workshop finds it. Every build also copies the libraries of the Plugins Workshop ships into `plugins` beside Workshop, where a development Workshop loads them.
- `cargo workshop bundle [--target <triple>] [--sign] [--gateway-binary <path>]` builds one platform's bundle in release with the Tauri CLI and prints the bundle's directory.

`cargo workshop --help` prints the full usage.

## The Gateway

The Gateway is its own product, in [cppalliance/runvil-gateway](https://github.com/cppalliance/runvil-gateway), so nothing here builds it. Every build uses the release `gateway-pin/pin.json` names. Stage it once per target:

```text
node tools/fetch-gateway-sidecar.mjs --target <triple>
```

The script downloads the release asset, checks its SHA-256 against the pin, and stages the executable in `crates/workshop/desktop/binaries/`, where every later build reuses it. `--gateway-binary <path>`, or the `RUNVIL_GATEWAY_SIDECAR` environment variable, names another Gateway instead; for a Windows target, remove the staged one first, because a named Gateway never replaces it. The bundle build runs the Gateway's `--version` and refuses one that is not the pinned version.

## The Plugins Workshop ships

Two library Plugins ship with Workshop, each a dynamic library whose code is in a repository of its own, pinned by commit in the root `Cargo.toml`:

- The std Plugin, from [cppalliance/runvil-plugin-std](https://github.com/cppalliance/runvil-plugin-std), holds the file tools. Its library is `plugin_std.dll`, `libplugin_std.so`, or `libplugin_std.dylib`.
- The bashkit Plugin, from [cppalliance/runvil-plugin-bashkit](https://github.com/cppalliance/runvil-plugin-bashkit), holds the shell. Its library is `plugin_bashkit.dll`, `libplugin_bashkit.so`, or `libplugin_bashkit.dylib`.

Nothing here builds them by name: the Workshop server depends on `workshop-plugin-std` and `workshop-plugin-bashkit`, so Cargo writes each library to `deps` in the profile's directory whenever it builds Workshop. Each mode takes the libraries from there. A build copies them into `plugins` beside Workshop in the target directory. A macOS or Linux bundle build has Tauri copy them into Workshop's bundle, at `Runvil.app/Contents/Resources/plugins` or at `usr/lib/runvil/plugins` in the AppImage, so the updater, which replaces only that bundle, replaces the libraries with it. The bundle build fails when the bundle Tauri built does not hold each library. A Workshop run with plain `cargo run -p workshop` has no Plugin directory, so its agents have no file tools and no shell; run `cargo workshop` once to place the libraries. Plain Cargo builds do not refresh the placed copies, so run `cargo workshop` again after a pin or the contract version changes.

`LIBRARIES` in `src/main/plugin.rs` names the two. A third library Plugin adds its name there and in `crates/workshop/desktop/installer.nsi`, and the install tests in `cppalliance/runvil` add it too.

## Building a bundle

Every platform needs Node 22, the staged Gateway above, and the UI install, which brings the pinned Tauri CLI:

```text
npm ci --prefix crates/workshop
```

The bundle lands in Tauri's bundle directory for the platform, `target/<triple>/release/bundle/<nsis|macos|appimage>/`. The build clears that directory first, so it holds one build's files, and prints it last as `bundle: <path>`:

- Windows, `bundle/nsis/`: the NSIS setup `Runvil_<version>_x64-setup.exe`, which Tauri builds from `crates/workshop/desktop/installer.nsi` with the Gateway inside. That template takes the Plugins' libraries from `deps` beside the built executable and installs them as `plugins\plugin_std.dll` and `plugins\plugin_bashkit.dll`. The setup is also Workshop's updater file, so `--sign` adds only `Runvil_<version>_x64-setup.exe.sig`.
- macOS, `bundle/macos/`: `Runvil.app`, with the Plugins' libraries inside it. `--sign` adds the updater file `Runvil.app.tar.gz` and `Runvil.app.tar.gz.sig`.
- Linux, `bundle/appimage/`: `Runvil_<version>_<amd64|aarch64>.AppImage`, with the Plugins' libraries inside it, beside `Runvil.AppDir`, the tree Tauri built it from. The AppImage is also Workshop's updater file, so `--sign` adds only its `.sig`.

Only the Windows setup holds the Gateway. The macOS and Linux installers place the Gateway beside Workshop, so those bundles do not hold it. The bundle build still needs the pinned Gateway on every platform, because it checks the version before it builds.

Linux also needs Tauri's build packages, the GStreamer plugins the AppImage bundles, and `patchelf`. On Ubuntu:

```text
gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-pulseaudio
gstreamer1.0-alsa patchelf
libwebkit2gtk-4.1-dev libssl-dev librsvg2-dev xdg-utils
```

Building the Intel macOS bundle on Apple silicon needs Rosetta, since the build runs the x86_64 Gateway to check its version.

Where FUSE is unavailable, such as in a container, set `APPIMAGE_EXTRACT_AND_RUN=1` so Tauri's AppImage tools run without it.

### Signing

Without `--sign`, the build applies `crates/workshop/desktop/tauri.unsigned.conf.json` and makes no updater files. With it, `tauri build` signs Workshop's updater files with the minisign key in `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. For a local try, make a throwaway key:

```text
node crates/workshop/node_modules/@tauri-apps/cli/tauri.js signer generate --ci -w <dir>/run.key
```

Installed copies trust only the release key, so files signed with any other key never install as an update.
