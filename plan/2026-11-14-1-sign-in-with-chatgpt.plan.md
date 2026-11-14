---
name: Sign In With ChatGPT In Workshop
overview: "Workshop presents the Gateway's plan proof on model calls, drives the ChatGPT sign-in from native commands that the page can call but cannot see inside, and moves the Gateway config page to its own origin so that page can never reach those commands. The UI is the least that makes the flow usable."
todos:
  - id: dropdown-headings
    content: "@workshop/look: a group with a heading in the dropdown menu, which the arrow keys skip"
    status: completed
  - id: plan-proof
    content: "workshop-gateway-discovery, workshop-gateway, workshop-server, workshop-support: read `gateway-private.json`, send the plan proof on the model list and on model rounds, contact a loopback Gateway directly, pin the Harness commit that carries the proof"
    status: completed
  - id: account-and-origin
    content: "workshop-gateway, workshop-server, the desktop app, the page: the ChatGPT account caller, six native commands behind a per-load capability, the status item and the picker rows, and the Gateway config page on a second loopback origin"
    status: completed
  - id: verify
    content: "run the checks each commit touches on that commit, and every check in AGENTS.md and the page's own checks on the last; drive the flow in the real window on the last"
    status: completed
isProject: false
---

# Sign in with ChatGPT in Workshop

## Why

The ChatGPT plan extension proposed for `runvil-gateway` lets a person sign in with their ChatGPT account and use the models their plan includes. The Gateway holds the credential. It serves a plan model only to a caller that presents the plan proof of its current boot, and it manages the sign-in only for a caller that presents the owner proof. Both proofs are in `gateway-private.json`, beside the discovery file.

Workshop is that caller. It needs three things:

- **The plan proof on model calls**, or the plan's models are never listed or served.
- **A way to sign in and out.** The owner proof must not reach the page, so the calls are native.
- **A page that cannot be used against those calls.** The Gateway config page was framed from the page's own origin, where a script has the page's DOM and its IPC. It moves to its own origin.

## The rule

- **Workshop keeps and manages no ChatGPT credential.** The Gateway owns the registration, the PKCE verifier, the tokens, and their life. Workshop's native side handles the authorization URL for a moment, to open the browser. It holds the two proofs in memory, redacted in `Debug`.
- **A proof is used only for the boot it names.** `gateway-private.json` is paired with `gateway.json` by `pid` and `epoch`, and a file that names another boot is ignored.
- **The plan proof goes on `GET /v1/models` and on model rounds, and nowhere else.**
- **The owner proof stays in the native side.** It is sent only to `127.0.0.1`, with no proxy and no redirect, and it is in no reply and no error.
- **The authorization URL never crosses IPC.** The native side opens the system browser itself.
- **The account commands answer only to the page's current load.** The page binds a random capability when it loads, the commands refuse any other caller, and a reload of the main page clears it.
- **The Gateway config page has its own origin.** It gets no IPC, it cannot navigate the top window, and the bridge takes messages only from that origin and that frame.
- **The server never forwards `/admin/chatgpt`.** The config page's admin forwarder refuses that path.
- **A loopback Gateway is contacted directly**, with no proxy and no redirect.

Decisions:

- **The account calls go from the desktop app to the Gateway**, not through `workshop-server`. The server answers the page's origin, and the owner proof must stay out of anything a page can reach.
- **The caller lives in `workshop-gateway`.** The desktop app keeps the capability, the browser opener, and the projection to what the page sees. Its `AGENTS.md` says the desktop app does not read Gateway configuration, and this keeps the HTTP calls out of it too.
- **The UI is text and is not a design.** Workshop's UI is early, and nothing here was modelled on another product. What OpenAI's guidelines ask for beyond this is listed under "Not built".
- **No logo.** OpenAI's Sign in with ChatGPT Terms, <https://openai.com/policies/sign-in-with-chatgpt-terms/>, say to use its names, logos, and buttons only as authorized. Whether a local open-source tool may use the mark is not settled.
- **The origin change lands with the account commands**, in one commit. Startup holds every config frame until the page's bind attempt has settled, and a reload of the main page clears the capability. Those two orderings span both parts, so neither part was built or tested without the other.

## Commit 1: group headings in the dropdown

- [dropdown.ts](../crates/workshop/look/dropdown.ts) and [dropdown.css](../crates/workshop/look/dropdown.css): a `DropdownGroup` renders as `.menu-group[role="group"]` with a `.menu-heading`. The heading is not an item, so the arrow keys skip it.
- Nothing uses it yet. The model picker does in commit 3.

## Commit 2: the plan proof and the direct loopback path

- [private.rs](../crates/workshop/gateway-discovery/src/private.rs): `GatewayPrivateFile::read` reads and validates `gateway-private.json`, and `names_boot_of` compares `pid` and `epoch` with the discovery file. `Debug` prints `[REDACTED]` for both proofs. [CONTRACT.md](../crates/workshop/gateway-discovery/CONTRACT.md) describes the file.
- [resolve.rs](../crates/workshop/gateway/src/resolve.rs) and [binding.rs](../crates/workshop/gateway/src/binding.rs): a resolved Gateway and its snapshot carry the plan proof of the boot they name, or none.
- [client.rs](../crates/workshop/gateway/src/client.rs): `GatewayClient` sends the proof on `GET /v1/models` only. For a loopback base URL its HTTP client uses no proxy and follows no redirect.
- [broker.rs](../crates/workshop/server/src/agents/broker.rs): `WorkshopBroker` passes the proof to the Harness's `GatewayBroker`, which sends it on model rounds.
- [loopback.rs](../crates/workshop/support/src/loopback.rs): `is_loopback_http_url` is the one predicate for "an http or https URL whose host is `localhost`, `127.0.0.0/8`, or `::1`". The Gateway client and the server's cross-site guard each had their own copy. The client's copy looked at the host alone, so it also said yes to `ws://127.0.0.1`. Its caller passes an http or https API root.
- `Cargo.toml`: the four Harness crates move to the commit that adds `GatewayBroker::with_plan_proof`.

## Commit 3: the account, the page, and the config page's origin

Native side:

- [account.rs](../crates/workshop/gateway/src/account.rs): `ChatgptAccount` makes the five `/admin/chatgpt` calls. Each call reads both files again and checks the boot before it builds a request. It returns the Gateway's status JSON, and for `enable_models` the status and body, as received. An error from the browser opener is discarded here, and a transport failure becomes a category here. `ChatgptAccountError` is `Copy` and holds closed categories and integers only, so no text from outside can ride in it.
- [chatgpt.rs](../crates/workshop/desktop/src/chatgpt.rs): six commands. `chatgpt_bind_capability` accepts the first non-empty capability while none is bound. `chatgpt_status`, `chatgpt_sign_in`, `chatgpt_sign_out`, `chatgpt_reset`, and `chatgpt_enable` require it.
- [capability.rs](../crates/workshop/desktop/src/chatgpt/capability.rs): `CapabilityGate`, with `reset_for_main_load`.
- [view.rs](../crates/workshop/desktop/src/chatgpt/view.rs): `AccountView` and the enable outcome are what the page gets. The Gateway's reply is read into typed fields here, so an unknown field is dropped, and a closed error becomes the words the page shows.

Page:

- [chatgpt-account.ts](../crates/workshop/ui/src/services/chatgpt-account.ts): the account service. It seals the capability in a closure, without replacing `invoke`, so a frozen `invoke` does not stop startup. It polls once a second only while a sign-in is waiting. It enables the plan's models once per flow, when the plan is granted and no plan model is listed yet. A late reply from an older flow is dropped.
- [chatgpt-status.ts](../crates/workshop/ui/src/parts/chrome/chatgpt-status.ts): a status bar item, `ChatGPT plan`, to the left of the meter and the LEDs, in a closed shadow root. Its popup has one action for the current state: `Sign out`, `Sign in again`, `Reset`, `Try again`, `Check again`, `Start over`, or `Dismiss`.
- [model-picker-trigger.ts](../crates/workshop/ui/src/parts/chrome/model-picker-trigger.ts): a `Sign in with ChatGPT` row when signed out, and the plan's models under one `ChatGPT plan` heading.
- [agent-panel.ts](../crates/workshop/ui/src/parts/agent/agent-panel.ts): a round that fails with a `chatgpt_` code makes the service read the status again and refresh the models. The server's relay appends that code to its failure label, and a new `refresh_models` socket frame re-reads the catalog.

The closed shadow root keeps the item's content out of ordinary DOM traversal. It is not a secrecy boundary inside the main page, which receives the account's email in `AccountView`.

The config page's origin:

- [serve.rs](../crates/workshop/server/src/serve.rs): a second listener on `127.0.0.1:0` serves only the proxied Gateway config documents, for `GET` and `HEAD` with a loopback `Host`. The main listener answers `404` on the old path and gains `GET /gateway/config-origin`. Shutdown waits for both listeners and the registered cleanup, and its watchdog can still force termination after the grace period.
- [csp.rs](../crates/workshop/server/src/csp.rs): the main page may frame only the config origin and may be framed by nothing. The config page may be framed only by the main origin, and its policy has no IPC source.
- [navigation.rs](../crates/workshop/desktop/src/navigation.rs) and [main.rs](../crates/workshop/desktop/src/main.rs): the config origin is allowed only at the panel's own URL. A top-level load of any other http or https origin is opened in the system browser and the window returns to the main page. A top-level load of the main page clears the capability. The window's IPC grant names the main origin alone.
- [gateway-config-panel.ts](../crates/workshop/ui/src/parts/gateway/gateway-config-panel.ts): the frame has `sandbox="allow-scripts allow-same-origin"` and no top-navigation permission. When the config origin cannot be read, no frame is mounted, and there is no fall back to the main origin.
- [gateway-config-bridge.ts](../crates/workshop/ui/src/parts/gateway/gateway-config-bridge.ts): a message is taken only when its origin is the config origin and its source is the live frame. Both are checked again after the forward returns, before the reply is posted to that origin.
- [main.ts](../crates/workshop/ui/src/main.ts): startup waits for the bind attempt to settle before it restores the layout, so no config frame exists while a bind is in flight. A bind that is refused leaves the account commands refusing, and the frame still mounts.

To take the origin change alone, these are its files: `server/src/{serve,csp,app,routes/gateway_config}.rs`, `desktop/src/navigation.rs`, the `config_url` and page-load parts of `desktop/src/main.rs`, and `ui/src/{parts/gateway/*,services/gateway-config-api.ts}`. Without the account commands the capability reset in `on_page_load` and the wait for the bind in `main.ts` have nothing to guard and drop out.

## Tests

- **Proofs.** `private-tests.rs` covers the file's validation, a boot mismatch, and the redaction. `client/tests.rs` and `broker-tests.rs` pin the header on the model list and on a round, and its absence without a proof.
- **Direct loopback.** [loopback_direct.rs](../crates/workshop/gateway/tests/loopback_direct.rs) runs the model list in a child process with `HTTP_PROXY` and `ALL_PROXY` set to a counting listener, after a canary proves the listener is consulted, and gives it a redirect whose target must receive nothing.
- **Account caller.** `account-tests.rs` covers the error type and the redaction, missing and mismatched files, the owner proof on a loopback request, and a redirect that is not followed.
- **Command helpers and projection.** `chatgpt-tests.rs` calls the helpers behind the commands against a local HTTP fixture, not Tauri's IPC dispatch: the two-request sign-in with the browser opener stubbed, the `409`, the error envelope on a `200`, an overlong code, and replies with wrong types or extra secret-looking fields, none of which reach the view. The capability gate has its own tests.
- **Server.** `gateway_config-tests.rs` covers the refusal of `/admin/chatgpt` for every method, the `404` on the main listener, and the method and `Host` guard. `serve-tests.rs` covers the two listeners and shutdown. `csp.rs` tests both policies.
- **Desktop.** `navigation.rs` tests the panel URL, a bare config URL, and the top-level classification. `main.rs` tests the IPC grant's configuration: it names the main origin alone.
- **Page.** `chatgpt-account.mjs`, `account-popover.mjs`, `status-indicators.mjs`, `model-picker-trigger.mjs`, and `workshop-dropdown.mjs` cover the service's states, polling, the one-time enable, stale replies, the popup, and the grouped menu. `gateway-config-bridge.mjs` covers messages from the main origin, a foreign origin, and a wrong source, and that the bind attempt settles before a frame mounts. `tauri-boot.mjs` covers the startup order with a frozen `invoke`.

## Not built

For whoever owns the UI. OpenAI's guidelines, <https://developers.openai.com/siwc/ui-ux-guidelines>, ask for three things that are not here:

- the wording `Using ChatGPT plan` near the composer or the model selector;
- a `Manage usage` link;
- a one-time welcome after the first sign-in.

Also not built: any logo, and any layout beyond the status item and the picker rows.

## Left alone

- **The owner proof on the config page's admin forwarder.** The Gateway does not require it on the older admin routes yet.
- **Windows and WebView2.** Not run. The Gateway saves no credential on Windows today.
- **A packaged build.** Not made.

## Verification

On the last commit, every command in [AGENTS.md](../AGENTS.md): format, both clippy sets with warnings denied, both nextest sets, the doc build, `cargo test -p build-xtask`, the wording check, and dylint. Also `cargo deny check`, `cargo audit`, and for the page `npm run typecheck`, `npm run build --workspace ui`, and `npm test` over the workspaces. Commit 1 changes only the page, so it got format and the page's three checks. Commit 2 changes only Rust, so it got format, both clippy sets, both nextest sets, the doc builds, `cargo test -p build-xtask`, and the wording check.

In the real window, on a debug build for macOS arm64, against a Gateway whose issuer is a local fake: sign in from the picker, see the plan's models appear, run one chat round, sign out, see the models go, and sign in again. That build is this branch plus a debug-only drive hook that is not in the branch. In drive mode `chatgpt_sign_in` records the authorization URL and does not open the browser, and the driver requests the fake's callback itself. So this shows the page, the native commands, and the Gateway working together. It does not show the system browser opening or a real consent screen. Then 26 checks around the config frame. The old path answers `404`. The frame is on its own origin and sandboxed. The main page cannot read it, and it cannot read the main page, reach IPC, or navigate the top window. The bridge forwards an admin status call and refuses `/admin/chatgpt/status` with `403`. A second bind and a wrong capability are refused. After a reload the account item is back and the Gateway saw no refused call. Both drivers and the fake issuer are outside this repository.

## What the build found

- **Three things fail on macOS with and without this change.** Nine page test files assert `Ctrl+` labels and chords. `realtime_relay::overload::stalled_browser_cleanup_is_bounded_after_gateway_disconnect` times out. At full parallelism most of the tests that start a Gateway time out, so the Workshop set was run two at a time.
- **A capture of the window was blank while another window covered it** on this machine, and was not blank once the window was in front. The window is brought to the front for each capture.
- **The check from inside the config frame saw no `__TAURI_INTERNALS__` at all.** It did not exercise a frame that has IPC and is then refused by the window's grant. The unit test pins the grant's configuration, and nothing here exercises that refusal at run time.
- **Run on macOS arm64 only.**
