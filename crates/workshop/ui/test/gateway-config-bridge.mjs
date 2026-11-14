// Unit test for the Gateway Config panel's workshop side: the
// window-level postMessage bridge (src/parts/gateway/gateway-config-bridge.ts) and
// the panel that embeds the iframe (src/parts/gateway/gateway-config-panel.ts).
// Bundles the TS modules with esbuild and drives them in jsdom. Covers:
// origin + source pinning (the iframe runs on its dedicated loopback
// config origin, so messages from the workshop origin, the gateway port,
// any foreign origin, or a spoofed event.source on the config origin are
// ignored and never forwarded), the ready announcement answered with a
// context message (theme + initial route) pinned to the config origin,
// the API-forward round trip through a stubbed /gateway/api server route
// (bearer stays server-side by construction), the transport-failure answer
// (status 0), action notifications landing on the status bar stub,
// startup-gate sequencing before loading the iframe, non-loopback /
// same-origin rejection on /gateway/config-origin, frame teardown on
// panel dispose, listener teardown on bridge dispose, and the panel's
// iframe address, sandbox, and title.
// Run: node test/gateway-config-bridge.mjs
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";
import { JSDOM } from "jsdom";

const uiDir = path.dirname(fileURLToPath(import.meta.url));

const result = await esbuild.build({
  stdin: {
    contents: `
      export { setupGatewayConfigBridge } from "./src/parts/gateway/gateway-config-bridge.ts";
      export { GatewayConfigPanel } from "./src/parts/gateway/gateway-config-panel.ts";
    `,
    resolveDir: path.join(uiDir, ".."),
    loader: "ts",
  },
  bundle: true,
  write: false,
  format: "esm",
  platform: "browser",
  target: "es2022",
  logLevel: "silent",
  // The panel imports its colocated CSS; strip it - the test drives only
  // the JS, and jsdom applies no stylesheets anyway.
  loader: { ".css": "empty" },
});
const code = result.outputFiles[0].text;
const { setupGatewayConfigBridge, GatewayConfigPanel } = await import(
  `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`
);

const WORKSHOP_ORIGIN = "http://127.0.0.1:7910";
const CONFIG_ORIGIN = "http://127.0.0.1:7911";
const GATEWAY_ORIGIN = "http://127.0.0.1:8081";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: `${WORKSHOP_ORIGIN}/`,
});
const { window } = dom;
globalThis.window = window;
globalThis.document = window.document;

const failures = [];
function check(name, condition) {
  if (!condition) failures.push(name);
}

/** Lets queued microtasks and zero-delay timers run. */
async function flush(turns = 10) {
  for (let i = 0; i < turns; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

// --- A stubbed workshop server: origin probe + proxy route -------------------

const fetched = [];
let originProbePayload = { origin: CONFIG_ORIGIN };
const fetchFn = async (url, init = {}) => {
  fetched.push({ url, init });
  if (url === "/gateway/config-origin") {
    return new Response(JSON.stringify(originProbePayload), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }
  if (url === "/gateway/api/admin/status") {
    return new Response(JSON.stringify({ profile: "default", models: [] }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }
  if (url === "/gateway/api/admin/system") {
    throw new TypeError("the workshop server is unreachable");
  }
  return new Response(JSON.stringify({ error: { code: "forward_denied" } }), {
    status: 403,
    headers: { "content-type": "application/json" },
  });
};
const proxyCalls = () => fetched.filter((call) => call.url.startsWith("/gateway/api"));
const originProbeCalls = () => fetched.filter((call) => call.url === "/gateway/config-origin");

const statusLines = [];
const statusBar = {
  showLocal: (label, severity) => statusLines.push({ label, severity }),
};

const activeFrameWindow = { id: "active-config-frame" };
const decoyFrameWindow = { id: "decoy-frame" };

const replies = [];
const bridge = setupGatewayConfigBridge({
  statusBar,
  fetchFn,
  win: window,
  reply: (_event, message, targetOrigin) => replies.push({ message, targetOrigin }),
  configOrigin: CONFIG_ORIGIN,
  frameSource: activeFrameWindow,
});

function dispatch(data, origin = CONFIG_ORIGIN, source = activeFrameWindow) {
  const event = new window.MessageEvent("message", { data, origin });
  Object.defineProperty(event, "source", { value: source });
  window.dispatchEvent(event);
}

// --- Origin + source pinning: foreign origins and wrong windows are ignored ---

dispatch({ type: "rv-api", id: "evil", method: "GET", path: "/admin/status", body: null }, "https://evil.example");
dispatch({ type: "rv-api", id: "same", method: "GET", path: "/admin/status", body: null }, WORKSHOP_ORIGIN);
dispatch({ type: "rv-bridge-ready" }, GATEWAY_ORIGIN);
dispatch({ type: "rv-api", id: "spoofed-src", method: "GET", path: "/admin/status", body: null }, CONFIG_ORIGIN, decoyFrameWindow);
dispatch({ type: "rv-bridge-ready" }, CONFIG_ORIGIN, null);
await flush();
check("foreign origins, the workshop origin, and spoofed source windows never forward", proxyCalls().length === 0);
check("foreign origins, the workshop origin, and spoofed source windows are never answered", replies.length === 0);

// --- Unknown or account message kinds are dropped even from the config frame ---

dispatch({ type: "chatgpt_status" }, CONFIG_ORIGIN, activeFrameWindow);
await flush();
check("unknown or account message types are dropped", replies.length === 0);

// --- The ready announcement is answered with the pinned config origin ---------

dispatch({ type: "rv-bridge-ready" });
await flush();
check("the ready announcement is answered", replies.length === 1);
check(
  "the answer is a context message holding theme and initial route",
  replies[0]?.message.type === "rv-context" &&
    replies[0]?.message.theme === "dark" &&
    replies[0]?.message.route === "#/local",
);
check(
  "the context reply pins the dedicated config origin, never * or workshop origin",
  replies[0]?.targetOrigin === CONFIG_ORIGIN,
);

// --- The API-forward round trip -----------------------------------------------

dispatch({ type: "rv-api", id: "r1", method: "GET", path: "/admin/status", body: null });
await flush();
check(
  "an api request forwards through the workshop server proxy route",
  proxyCalls().length === 1 && proxyCalls()[0].url === "/gateway/api/admin/status",
);
const apiReply = replies.find((entry) => entry.message.type === "rv-api-result");
check(
  "the proxy's answer comes back with the request id, status, and body",
  apiReply !== undefined &&
    apiReply.message.id === "r1" &&
    apiReply.message.status === 200 &&
    apiReply.message.contentType === "application/json" &&
    apiReply.message.body.includes('"profile":"default"'),
);
check("the api reply pins the config origin", apiReply?.targetOrigin === CONFIG_ORIGIN);

// --- A transport failure answers status 0 --------------------------------------

dispatch({ type: "rv-api", id: "r2", method: "GET", path: "/admin/system", body: null });
await flush();
const failureReply = replies.find((entry) => entry.message.id === "r2");
check(
  "an unreachable workshop server answers status 0, not silence",
  failureReply !== undefined && failureReply.message.status === 0,
);

// --- Action notifications land on the status bar --------------------------------

dispatch({ type: "rv-action", action: "apply" });
dispatch({ type: "rv-action", action: "download-started" });
dispatch({ type: "rv-action", action: "reboot-the-machine" });
await flush();
check(
  "apply and download-started reach the status bar as local info lines",
  statusLines.map((line) => line.label).join("|") ===
    "Gateway configuration applied|Gateway download started" &&
    statusLines.every((line) => line.severity === "info"),
);

// --- Panel startup sequencing, isolated origin URL, and dispose detach --------

{
  let releaseGate;
  const beforeLoad = new Promise((resolve) => {
    releaseGate = resolve;
  });
  const panel = new GatewayConfigPanel({
    workshopOrigin: WORKSHOP_ORIGIN,
    beforeLoad,
    fetchFn,
    frameBridge: bridge,
  });
  window.document.body.append(panel.element);
  panel.init({ params: {} });
  check(
    "no iframe element exists in the DOM before the startup bind gate settles",
    panel.element.querySelector("iframe") === null,
  );
  check("the iframe waits for the startup bind gate before probing config-origin", originProbeCalls().length === 0);

  releaseGate();
  await flush();

  const iframe = panel.element.querySelector("iframe");
  check("the panel mounts the iframe only after startup bind settles", iframe !== null);
  check("the panel fetches /gateway/config-origin once the gate settles", originProbeCalls().length === 1);
  check(
    "the iframe loads the config SPA on its dedicated loopback config origin",
    iframe?.getAttribute("src") ===
      `${CONFIG_ORIGIN}/?mode=panel&bridge=${encodeURIComponent(WORKSHOP_ORIGIN)}`,
  );
  const sandbox = iframe?.getAttribute("sandbox") ?? "";
  check(
    "the iframe sandbox grants scripts and same-origin only, without allow-top-navigation",
    sandbox === "allow-scripts allow-same-origin" &&
      !sandbox.includes("allow-top-navigation") &&
      !sandbox.includes("allow-top-navigation-by-user-activation"),
  );
  check("the iframe has an accessible title", iframe?.getAttribute("title") === "Gateway Config");

  const repliesAfterPanelAttach = replies.length;
  dispatch({ type: "rv-bridge-ready" }, CONFIG_ORIGIN, activeFrameWindow);
  await flush();
  check(
    "the prior non-DOM window is no longer accepted once the live panel attaches",
    replies.length === repliesAfterPanelAttach,
  );

  dispatch({ type: "rv-bridge-ready" }, CONFIG_ORIGIN, iframe.contentWindow);
  await flush();
  check(
    "the attached live iframe contentWindow passes source validation and gets a pinned reply",
    replies.length === repliesAfterPanelAttach + 1 && replies.at(-1)?.targetOrigin === CONFIG_ORIGIN,
  );

  const liveWindow = iframe.contentWindow;
  panel.dispose();
  const repliesBeforeDisposedSend = replies.length;
  dispatch({ type: "rv-bridge-ready" }, CONFIG_ORIGIN, liveWindow);
  await flush();
  check("disposing the panel detaches the frame from the bridge", replies.length === repliesBeforeDisposedSend);
}

// --- Invalid config origins from the probe never load the iframe --------------

for (const badOrigin of [WORKSHOP_ORIGIN, "https://evil.example", `${CONFIG_ORIGIN}/extra`]) {
  originProbePayload = { origin: badOrigin };
  const panel = new GatewayConfigPanel({
    workshopOrigin: WORKSHOP_ORIGIN,
    beforeLoad: Promise.resolve(),
    fetchFn,
    frameBridge: bridge,
  });
  panel.init({ params: {} });
  await flush();
  const iframe = panel.element.querySelector("iframe");
  check(`bad config origin (${badOrigin}) never mounts an iframe or sets src`, iframe === null);
  panel.dispose();
}

// --- Dispose detaches the listener ----------------------------------------------

const repliesBefore = replies.length;
bridge.dispose();
dispatch({ type: "rv-bridge-ready" });
await flush();
check("a disposed bridge answers nothing", replies.length === repliesBefore);

if (failures.length > 0) {
  console.error(`gateway-config-bridge: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("gateway-config-bridge: all assertions passed");
