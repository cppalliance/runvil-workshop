// The ChatGPT account surface: one status value, the picker item, the
// closed-shadow status element, and the seal that keeps a same-origin
// config frame from calling the account commands or reading the status.
// Run: node test/chatgpt-account.mjs
import { writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";
import { JSDOM } from "jsdom";
import { assertNoLeaks } from "./helpers/leak-check.mjs";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const bundle = await esbuild.build({
  stdin: {
    contents: `
      export * as lifecycle from "@workshop/platform/lifecycle";
      export { ModelService } from "./src/services/model-service.ts";
      export { ModelPickerTrigger } from "./src/parts/chrome/model-picker-trigger.ts";
      export { ChatgptStatus } from "./src/parts/chrome/chatgpt-status.ts";
      export {
        ChatgptAccount, sealAccountInvoke, selectionAfterRefresh, isSignInRoundFailure, accountChip,
      } from "./src/services/chatgpt-account.ts";
    `,
    resolveDir: path.join(testDir, ".."),
    loader: "ts",
  },
  bundle: true,
  write: false,
  format: "esm",
  platform: "browser",
  target: "es2022",
  logLevel: "silent",
  loader: { ".css": "empty" },
});

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://127.0.0.1:7910/" });
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.HTMLElement = dom.window.HTMLElement;
globalThis.HTMLButtonElement = dom.window.HTMLButtonElement;
globalThis.Element = dom.window.Element;
globalThis.Node = dom.window.Node;
globalThis.CustomEvent = dom.window.CustomEvent;
globalThis.ShadowRoot = dom.window.ShadowRoot;

const bundlePath = path.join(os.tmpdir(), "runvil-chatgpt-account-test.mjs");
await writeFile(bundlePath, bundle.outputFiles[0].text);
const {
  lifecycle,
  ModelService,
  ModelPickerTrigger,
  ChatgptStatus,
  ChatgptAccount,
  sealAccountInvoke,
  selectionAfterRefresh,
  isSignInRoundFailure,
  accountChip,
} = await import(pathToFileURL(bundlePath).href);

const failures = [];
function check(name, condition) {
  if (!condition) failures.push(name);
}

const signedOut = { state: "signed_out", email: null, planGranted: false, detail: "Signed out", planModels: [] };
const waiting = {
  state: "waiting",
  email: "a@example.com",
  planGranted: false,
  detail: "Waiting for the browser. Quitting Workshop stops the Gateway, and the browser page will then fail.",
  planModels: [],
};
const signedIn = {
  state: "signed_in",
  email: "a@example.com",
  planGranted: true,
  detail: "Signed in",
  planModels: ["gpt-6.1-sol-chatgpt"],
};

function backend(view) {
  const calls = [];
  return {
    calls,
    status: async () => view,
    signIn: async () => {
      calls.push("signIn");
      return waiting;
    },
    signOut: async () => signedOut,
    reset: async () => signedOut,
    enable: async () => ({ endpoint: "chatgpt-plan", added: [], renamed: [], skipped: [] }),
  };
}

function accountFor(view, models = [], current = "") {
  const service = new ModelService(() => true);
  service.setModels(models);
  if (current !== "") service.applySelected(current);
  const api = backend(view);
  const account = new ChatgptAccount(
    api,
    () => {
      api.calls.push("refresh");
      return true;
    },
    (id) => service.setCurrent(id),
    () => service.models,
    () => service.current,
  );
  return { account, service, api };
}

const FAST_POLL_MS = 15;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

await assertNoLeaks(lifecycle, async () => {
  {
    const kept = selectionAfterRefresh("gpt-6.1-sol", [{ id: "gpt-6.1-sol" }, { id: "gpt-6.1-sol-chatgpt" }], ["gpt-6.1-sol-chatgpt"]);
    check("an existing selection is left alone", kept === null);
    const picked = selectionAfterRefresh("", [{ id: "gpt-6.1-sol" }, { id: "gpt-6.1-sol-chatgpt" }], ["gpt-6.1-sol-chatgpt"]);
    check("an empty selection takes the first plan model", picked === "gpt-6.1-sol-chatgpt");
    check("a sign-in failure is the chatgpt code", isSignInRoundFailure("Gateway error: 403 Forbidden (chatgpt_sign_in_required)"));
    check("another failure is not a sign-in failure", !isSignInRoundFailure("Gateway error: 500 Internal Server Error"));
  }

  {
    const { account, service, api } = accountFor(waiting);
    await account.signIn();
    check("sign-in asks the backend", api.calls.includes("signIn"));
    const mount = document.createElement("div");
    document.body.append(mount);
    const status = new ChatgptStatus(account);
    mount.append(status.element);
    await account.signIn();
    const frame = document.createElement("iframe");
    document.body.append(frame);
    const probe = frame.contentWindow;
    const statusEl = probe.parent.document.querySelector(".ws-chatgpt-status");
    check("the frame cannot open the shadow", statusEl.shadowRoot === null);
    check("the outer span text is empty", statusEl.textContent === "");
    check("the email is not in the outer span html", !statusEl.innerHTML.includes("a@example.com"));
    const flags = { enabling: false, enableProblem: null, dismissedCleanup: false };
    check("a clean sign-out hides the item", accountChip(signedOut, flags).hidden);
    const ready = accountChip(signedIn, flags);
    check("a signed-in item is the two words", ready.label === "ChatGPT plan" && ready.tooltip === "a@example.com" && ready.button === "sign-out" && !ready.warn);
    const expired = accountChip({ state: "needs_sign_in", email: "long-address@example.com", planGranted: false, detail: "expired", planModels: [] }, flags);
    check("an expired sign-in keeps the two words and the short line", expired.label === "ChatGPT plan" && expired.warn && expired.tooltip === "Sign-in expired." && expired.button === "sign-in-again");
    const pending = accountChip(waiting, flags);
    check("waiting keeps the two words and Start over", pending.label === "ChatGPT plan" && !pending.warn && pending.tooltip === "Finish sign-in in browser." && pending.button === "start-over");
    const broken = accountChip({ state: "unreadable", email: null, planGranted: false, detail: "unreadable", planModels: [] }, flags);
    check("an unreadable sign-in keeps the two words and Reset", broken.label === "ChatGPT plan" && broken.warn && broken.tooltip === "Saved sign-in is unreadable." && broken.button === "reset");
    status.dispose();
    account.dispose();
    service.dispose();
    frame.remove();
    mount.remove();
  }

  {
    const calls = [];
    const internals = {};
    function raw(command, args) {
      calls.push({ command, capability: args?.capability });
      return Promise.resolve(signedOut);
    }
    Object.defineProperty(internals, "invoke", { value: raw });
    let threw = false;
    let accountCall;
    try {
      accountCall = sealAccountInvoke(internals);
    } catch {
      threw = true;
    }
    check("a non-writable invoke is not replaced", !threw && internals.invoke === raw);
    const frame = document.createElement("iframe");
    document.body.append(frame);
    frame.contentWindow.__TAURI_INTERNALS__ = internals;
    await frame.contentWindow.__TAURI_INTERNALS__.invoke("chatgpt_sign_in");
    check("the frame's call carries no capability", calls[0]?.command === "chatgpt_sign_in" && calls[0]?.capability === undefined);
    await accountCall("chatgpt_status");
    check("the account caller still reaches invoke", calls[1]?.command === "chatgpt_status" && typeof calls[1]?.capability === "string");
    frame.remove();
  }

  {
    const service = new ModelService(() => true);
    service.setModels([{ id: "gpt-6.1-sol" }]);
    const api = backend(signedOut);
    const account = new ChatgptAccount(api, () => true, (id) => service.setCurrent(id), () => service.models, () => service.current);
    const trigger = new ModelPickerTrigger(service, account);
    document.body.append(trigger.element);
    trigger.element.click();
    const items = [...document.querySelectorAll(".menu-item")].map((item) => item.textContent);
    check("the sign-in item stays beside existing models", items[0] === "Sign in with ChatGPT" && items.includes("gpt-6.1-sol"));
    document.querySelector(".menu-item").click();
    await Promise.resolve();
    check("the menu item starts a sign-in", api.calls.includes("signIn"));
    trigger.dispose();
    account.dispose();
    service.dispose();
    trigger.element.remove();
  }

  {
    const json = JSON.stringify(signedIn);
    for (const needle of ["authorize_url", "authorizeUrl", "owner_proof", "ownerProof", "plan_proof", "api_key", "access_token", "refresh_token"]) {
      check(`the view json omits ${needle}`, !json.includes(needle));
    }
  }

{
  const pending = {
    state: "signed_in",
    email: "a@example.com",
    planGranted: true,
    detail: "Signed in",
    planModels: ["gpt-6.1-sol-chatgpt"],
    accountStatus: "active",
    cleanupUnconfirmed: true,
  };
  const clear = { ...pending, cleanupUnconfirmed: false };
  const views = [clear, pending];
  let index = 0;
  const api = backend(pending);
  api.status = async () => views[Math.min(index++, views.length - 1)];
  const account = new ChatgptAccount(api, () => true, () => true, () => [{ id: "gpt-6.1-sol-chatgpt" }], () => "gpt-6.1-sol-chatgpt");
  account.dismissCleanup();
  await account.refresh();
  await account.refresh();
  check(
    "a later cleanup shows after the earlier one cleared",
    account.chip.warn && account.chip.tooltip === "Remote sign-out unconfirmed." && account.chip.button === "sign-out",
  );
  account.dispose();
}

{
  const flags = { enabling: false, enableProblem: null, dismissedCleanup: false };
  const active = {
    state: "signed_in",
    email: "a@example.com",
    planGranted: true,
    detail: "Signed in",
    planModels: ["gpt-6.1-sol-chatgpt"],
    accountStatus: "active",
    cleanupUnconfirmed: true,
  };
  const shown = accountChip(active, flags);
  check(
    "an active account still shows an unconfirmed cleanup",
    shown.label === "ChatGPT plan" && shown.warn && shown.tooltip === "Remote sign-out unconfirmed." && shown.button === "sign-out",
  );
  const dismissed = accountChip(active, { ...flags, dismissedCleanup: true });
  check("dismissing the cleanup restores the address tooltip", dismissed.tooltip === "a@example.com" && !dismissed.warn && dismissed.button === "sign-out");
}

{
  const listeners = new Set();
  const onModels = (listener) => {
    listeners.add(listener);
    return { dispose() { listeners.delete(listener); } };
  };
  const pushModels = (models) => {
    for (const listener of [...listeners]) listener(models);
  };
  const granted = {
    state: "signed_in",
    email: "a@example.com",
    planGranted: true,
    detail: "Signed in",
    planModels: [],
    accountStatus: "active",
  };
  const withModels = { ...granted, planModels: ["gpt-6.1-sol-chatgpt"] };
  let enableCalls = 0;
  let statusCalls = 0;
  const api = {
    calls: [],
    status: async () => {
      statusCalls += 1;
      return enableCalls === 0 ? granted : withModels;
    },
    signIn: async () => waiting,
    signOut: async () => signedOut,
    reset: async () => signedOut,
    enable: async () => {
      enableCalls += 1;
      return { endpoint: "chatgpt-plan", added: ["gpt-6.1-sol-chatgpt"], renamed: [], skipped: [] };
    },
  };
  const selected = [];
  let models = [];
  const account = new ChatgptAccount(
    api,
    () => {
      api.calls.push("refresh");
      models = [{ id: "gpt-6.1-sol-chatgpt" }];
      pushModels(models);
      return true;
    },
    (id) => {
      selected.push(id);
      return true;
    },
    () => models,
    () => "",
    onModels,
    FAST_POLL_MS,
  );
  await account.signIn();
  check("waiting does not enable before the browser finishes", enableCalls === 0);
  await sleep(60);
  check("a finished sign-in enables once", enableCalls === 1);
  check("the catalog is refreshed for every window", api.calls.includes("refresh"));
  check("the first plan model is selected after the catalog arrives", selected.at(-1) === "gpt-6.1-sol-chatgpt");
  const polls = statusCalls;
  await sleep(60);
  check("polling stops on the signed-in state", statusCalls === polls);
  account.dispose();
}

{
  let statusCalls = 0;
  let enableCalls = 0;
  const api = backend(waiting);
  api.status = async () => {
    statusCalls += 1;
    return { state: "needs_sign_in", email: "a@example.com", planGranted: false, detail: "expired", planModels: [] };
  };
  api.enable = async () => {
    enableCalls += 1;
    return { endpoint: "chatgpt-plan", added: [], renamed: [], skipped: [] };
  };
  const account = new ChatgptAccount(api, () => true, () => true, () => [], () => "", undefined, FAST_POLL_MS);
  await account.signIn();
  await sleep(60);
  check("an expired sign-in is not enabled", enableCalls === 0);
  const polls = statusCalls;
  await sleep(60);
  check("polling stops on an expired sign-in", statusCalls === polls);
  account.dispose();
}

{
  let release = () => {};
  const held = new Promise((resolve) => {
    release = resolve;
  });
  let reads = 0;
  const api = backend(signedOut);
  api.status = () => {
    reads += 1;
    return reads === 1 ? held : Promise.resolve(signedOut);
  };
  const account = new ChatgptAccount(api, () => true, () => true, () => [], () => "");
  const refreshing = account.refresh();
  await Promise.resolve();
  await account.signOut();
  release(signedIn);
  await refreshing;
  check("a stale refresh does not restore a signed-in view", account.snapshot.state === "signed_out");
  account.dispose();
}

{
  const listeners = new Set();
  const onModels = (listener) => {
    listeners.add(listener);
    return { dispose() { listeners.delete(listener); } };
  };
  const selected = [];
  const api = backend({ ...signedIn, planModels: ["gpt-6.1-sol-chatgpt"] });
  const account = new ChatgptAccount(
    api,
    () => true,
    (id) => {
      selected.push(id);
      return true;
    },
    () => [],
    () => "",
    onModels,
  );
  await account.refresh();
  await account.signOut();
  for (const listener of [...listeners]) listener([{ id: "gpt-6.1-sol-chatgpt" }]);
  check("a catalog that arrives after sign-out does not select a plan model", selected.length === 0);
  account.dispose();
}

{
  const flush = async () => {
    for (let i = 0; i < 8; i += 1) await Promise.resolve();
  };
  let releaseFirst = () => {};
  let releaseSecond = () => {};
  const first = new Promise((resolve) => {
    releaseFirst = resolve;
  });
  const second = new Promise((resolve) => {
    releaseSecond = resolve;
  });
  let enableCalls = 0;
  const granted = { state: "signed_in", email: "a@example.com", planGranted: true, detail: "Signed in", planModels: [] };
  const api = backend(signedOut);
  api.enable = () => {
    enableCalls += 1;
    return enableCalls === 1 ? first : second;
  };
  api.signIn = async () => granted;
  api.status = async () => ({ ...granted, planModels: enableCalls >= 2 ? ["gpt-6.1-sol-chatgpt"] : [] });
  const account = new ChatgptAccount(api, () => true, () => true, () => [], () => "");
  const firstEnable = account.enable();
  await flush();
  const signingIn = account.signIn();
  await flush();
  check("a new sign-in starts its own enable while the old one is held", enableCalls === 2);
  check("the new flow is busy adding models", account.chip.tooltip === "Adding plan models...");
  releaseFirst();
  await firstEnable;
  check("the stale enable does not clear the new flow's busy flag", enableCalls === 2 && account.chip.tooltip === "Adding plan models...");
  releaseSecond();
  await signingIn;
  check("the new enable finishes on the granted account", account.snapshot.state === "signed_in" && account.chip.tooltip !== "Adding plan models...");
  account.dispose();
}
});

if (failures.length > 0) {
  console.error(`chatgpt-account: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("chatgpt-account: all assertions passed");
