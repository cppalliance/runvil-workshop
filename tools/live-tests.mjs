// Runs the ten live tests of `workshop-server`, against Gateways that this
// script and the tests start and stop.
//
// Usage: node tools/live-tests.mjs [--workshop <path>] [--gateway <path>]
//   --workshop <path>  the Workshop program; default
//                      target/debug/runvil-workshop, where `cargo workshop`
//                      builds it
//   --gateway <path>   the Gateway program; default the `runvil-gateway`
//                      beside the Workshop program, where `cargo workshop`
//                      places the pinned release
//
// Environment:
//   CHEAPSEEK_BASE_URL  the base URL of a running CheapSeek, such as
//                       http://127.0.0.1:54321/v1
//
// CheapSeek (https://github.com/cppalliance/cheapseek) is a test server that
// speaks the OpenAI chat API and computes each reply from fixed rules, so
// the tests need neither a model file nor a provider's key.
//
// The live tests are ignored in a normal test run, because each needs a
// Gateway in a certain state. This script gives each group its state:
//
// 1. With a Gateway running: the six tests of `live`, the test of
//    `live_program`, and the typing test of `live_window`.
// 2. With that Gateway stopped: the test of `live_stop` and the closing test
//    of `live_window`, each of which starts a Gateway itself.
//
// The script makes a temporary home folder whose Gateway config serves
// CheapSeek as its one chat model on a free loopback port, and hands the
// tests that folder, the address, the key, and the two programs through
// the `RUNVIL_E2E_*` variables their modules document. It stops its
// Gateway before every exit.
//
// Runs on Linux. Needs Node 22 or later, cargo-nextest, `tauri-driver`
// (`cargo install tauri-driver --locked`), `WebKitWebDriver`, `xvfb-run`,
// `dbus-run-session`, `setsid`, `lsof`, and `pkill`.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, writeFileSync, writeSync } from "node:fs";
import { createConnection, createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
// The key the Gateway requires of its callers. The Gateway listens on the
// loopback address only, for the length of one run.
const KEY = "live-tests";
const TOOLS = ["cargo", "tauri-driver", "WebKitWebDriver", "xvfb-run", "dbus-run-session", "setsid", "lsof", "pkill"];

// The Gateway this script started, while it runs.
let gateway;

// The Gateway runs in a process group of its own, because the program this
// script starts supervises a second process that serves.
function stop() {
  if (gateway) {
    try {
      process.kill(-gateway.pid, "SIGTERM");
    } catch {
      // The group is gone already.
    }
    gateway = undefined;
  }
}

// The line is written before the exit: a line queued on a pipe can be lost
// when the process exits.
function fail(message) {
  writeSync(2, `live-tests: ${message}\n`);
  stop();
  process.exit(1);
}

// Whatever ends the script, its Gateway is stopped first.
process.on("uncaughtException", (error) => fail(`stopped by an error: ${error?.stack ?? error}`));
process.on("unhandledRejection", (reason) => fail(`stopped by an error: ${reason?.stack ?? reason}`));
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => fail(`stopped by ${signal}`));
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function freePort() {
  return new Promise((done) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });
}

// Whether a program accepts connections on the loopback port.
function listening(port) {
  return new Promise((done) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    socket.once("connect", () => {
      socket.destroy();
      done(true);
    });
    socket.once("error", () => done(false));
  });
}

// The value of `--name <value>` among the arguments, or `fallback`.
function option(name, fallback) {
  const at = process.argv.indexOf(name);
  if (at === -1) return fallback;
  const value = process.argv[at + 1];
  if (!value) fail(`${name} must be followed by a path`);
  return resolve(value);
}

if (process.platform !== "linux") fail(`live-tests.mjs runs on Linux; this is ${process.platform}`);
const known = new Set(["--workshop", "--gateway"]);
for (let at = 2; at < process.argv.length; at += 2) {
  if (!known.has(process.argv[at])) {
    fail(`usage: live-tests.mjs [--workshop <path>] [--gateway <path>]; got '${process.argv[at]}'`);
  }
}
const workshop = option("--workshop", join(ROOT, "target", "debug", "runvil-workshop"));
const gatewayProgram = option("--gateway", join(dirname(workshop), "runvil-gateway"));
if (!existsSync(workshop)) fail(`the Workshop program ${workshop} does not exist; build it with \`cargo workshop\``);
if (!existsSync(gatewayProgram)) {
  fail(`the Gateway program ${gatewayProgram} does not exist; \`cargo workshop\` places the pinned release there`);
}
if (dirname(gatewayProgram) !== dirname(workshop)) {
  fail(`the Gateway program must be beside the Workshop program, where Workshop looks for it; it is ${gatewayProgram}`);
}
const cheapseek = process.env.CHEAPSEEK_BASE_URL;
if (!cheapseek) fail("CHEAPSEEK_BASE_URL must hold the base URL of a running CheapSeek; it is empty");
for (const tool of TOOLS) {
  if (spawnSync("sh", ["-c", `command -v ${tool}`]).status !== 0) fail(`${tool} must be on PATH; it is not`);
}

const home = mkdtempSync(join(tmpdir(), "runvil-live-"));
const port = await freePort();
const root = `http://127.0.0.1:${port}/v1`;
mkdirSync(join(home, ".runvil"), { recursive: true });
// `trust_loopback` is off, so that the test of the key sees a request
// without one refused.
writeFileSync(
  join(home, ".runvil", "gateway.toml"),
  `config-version = 0

[server]
bind = "127.0.0.1:${port}"
api_key = "${KEY}"
trust_loopback = false

[[endpoint]]
id = "cheapseek"
protocol = "openai"
base_url = "${cheapseek}"
api_key = ""

[[model]]
name = "cheapseek"
description = "CheapSeek, a test server that answers by rule"
context = 32768
upstream = "cheapseek"
endpoints = ["cheapseek"]

[[profile]]
name = "default"
models = []
`,
);

// The closing test has Workshop start the Gateway, which Workshop does
// without naming a profile. With no profile selected, the Gateway serves
// CheapSeek and Workshop's model picker still shows no model, so the state
// file selects the profile.
writeFileSync(join(home, ".runvil", "gateway.state.toml"), 'active_profile = "default"\n');

const env = {
  ...process.env,
  RUNVIL_E2E_GATEWAY: root,
  RUNVIL_E2E_KEY: KEY,
  RUNVIL_E2E_GATEWAY_HOME: home,
  RUNVIL_E2E_GATEWAY_BIN: gatewayProgram,
  RUNVIL_E2E_WORKSHOP_BIN: workshop,
};

// Runs the ignored tests that `filter` selects, one at a time, and returns
// whether all of them passed.
function tests(filter) {
  const run = spawnSync(
    "cargo",
    ["nextest", "run", "--locked", "-p", "workshop-server", "--run-ignored", "ignored-only", "-j", "1", "--no-fail-fast", "-E", filter],
    { cwd: ROOT, stdio: "inherit", env },
  );
  return run.status === 0;
}

// 1. The tests that need a running Gateway.
const log = join(home, "gateway.log");
const output = openSync(log, "a");
gateway = spawn(gatewayProgram, ["--no-tray", "--profile", "default"], {
  env: { ...process.env, HOME: home },
  cwd: home,
  detached: true,
  stdio: ["ignore", output, output],
});
const started = gateway;
let listed = "";
const ready = Date.now() + 120_000;
while (!listed.includes('"cheapseek"')) {
  if (started.exitCode !== null) fail(`the Gateway exited with ${started.exitCode} while starting; see ${log}`);
  if (Date.now() > ready) fail(`the Gateway must list the model cheapseek within 120 seconds; see ${log}`);
  await sleep(500);
  try {
    const response = await fetch(`${root}/models`, {
      headers: { authorization: `Bearer ${KEY}` },
      signal: AbortSignal.timeout(5_000),
    });
    listed = await response.text();
  } catch {
    // The Gateway does not listen yet.
  }
}
console.log(`live-tests: a Gateway serves CheapSeek at ${root}`);
const withGateway = tests("test(live::) | test(live_program::) | test(typed_in_the_workshop_window)");

// 2. The tests that start a Gateway themselves, at the same address.
stop();
const free = Date.now() + 60_000;
while (await listening(port)) {
  if (Date.now() > free) fail(`the Gateway must stop listening on port ${port} within 60 seconds; see ${log}`);
  await sleep(500);
}
console.log("live-tests: the Gateway is stopped");
const killed = tests("test(live_stop::)");
const closed = tests("test(closing_workshops_window)");

if (!(withGateway && killed && closed)) {
  fail(
    `every group must pass; with a running Gateway: ${withGateway ? "passed" : "failed"}, ` +
      `the kill test: ${killed ? "passed" : "failed"}, the closing test: ${closed ? "passed" : "failed"}`,
  );
}
writeSync(1, "live-tests: all three groups passed\n");
process.exit(0);
