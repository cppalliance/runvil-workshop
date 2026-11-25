---
name: Pin the Gateway client adapter
overview: Move the pin of runvil, harness-gateway-client, and plugin-mcp to the Harness commit whose harness-gateway-client is an adapter over gateway-client, allow the gateway-client git source in deny.toml, and refresh Cargo.lock. Workshop's source does not change, because the adapter still exports every name Workshop uses.
todos:
  - id: workshop-pr
    content: "Pin the three Harness crates at one commit, allow the gateway-client git source, refresh Cargo.lock; pull request from the fork, CI, fast-forward merge"
    status: pending
isProject: false
---

# Pin the Gateway client adapter

This is the part of the extraction plan that concerns this repository. `harness-gateway-client` is now an adapter over `gateway-client`, a repository of its own, and it keeps its name and the names Workshop uses. This change moves the pin of the three Harness crates to the Harness commit that holds the adapter, and it allows the new git source in `deny.toml`. It changes no Workshop source.

### runvil-workshop (step D1)

- **`Cargo.toml`, lines 28 to 30.** `runvil`, `harness-gateway-client`, and `plugin-mcp` all get `rev` = H. All three name one `rev`, as its `harness_pin` check requires.
- **`Cargo.lock`.** `cargo update -p harness-gateway-client` moves the Harness's git source to H and adds `gateway-client` as a git package. Commit the lock.
- **`deny.toml`, `allow-git`.** Add `"https://github.com/cppalliance/gateway-client"`. The lock now holds that source through `harness-gateway-client`, and `unknown-git = "deny"` refuses it otherwise. Today the list names the Harness, `runvil-plugin-api`, `runvil-plugin-std`, `runvil-plugin-bashkit`, and `runvil-plugin-web`.
- **No source change.** Workshop names `GatewayBroker`, `TokioTimer`, `provide_gateway_resources`, `GatewayEndpoint`, and `SecretString`, and the adapter still exports all five.
- **Checks:** the commands in its `AGENTS.md` verification list, which include `cargo test -p build-xtask` (the `harness_pin` check) and `cargo deny check`.
