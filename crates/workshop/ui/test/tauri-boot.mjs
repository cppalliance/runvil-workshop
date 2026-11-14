// A desktop webview whose `__TAURI_INTERNALS__.invoke` cannot be replaced
// must still mount the workbench. Tauri 2.11 defines that property with
// only a value, so it is neither writable nor configurable.
// Run: node test/tauri-boot.mjs (after `node build.mjs`).
import { bootWorkbench } from "./helpers/boot.mjs";

await bootWorkbench(
  "a frozen Tauri invoke still mounts the workbench",
  async (ctx) => {
    const { document, failures } = ctx;
    if (!document.querySelector("#dock .ws-workshop-tree")) {
      failures.push("the Workshop tree did not mount");
    }
    if (!document.querySelector("#dock .ws-agent-panel")) {
      failures.push("the agent panel did not mount");
    }
  },
  { frozenInvoke: true },
);
