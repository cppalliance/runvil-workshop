// Unit test for the pluggable menu system (src/ui/menu/command-registry.ts,
// menu-registry.ts, menu-renderer.ts): the window-menu.ts split. Commands
// are descriptors keyed by id in the command registry; menu placements are
// items per menu id in the menu registry; the renderer reads both and
// knows nothing about what is registered. Bundles the three modules with
// esbuild and drives them against jsdom built from the real index.html.
// Covers: command registration, lookup, execution, upsert, and disposal;
// menu ordering, item upsert keeping position, and provider registration;
// the renderer building popovers from the registries, dispatching rows,
// keyboard dismissal, dynamic provider rows rebuilding on open and on
// change while open, and the missing-button diagnostic.
// Run: node test/menu-registries.mjs
import { readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";
import { JSDOM } from "jsdom";

const uiDir = path.dirname(fileURLToPath(import.meta.url));
const html = await readFile(path.join(uiDir, "..", "index.html"), "utf8");

const bundle = await esbuild.build({
  stdin: {
    contents: `
      export { CommandRegistry } from "./src/ui/menu/command-registry.ts";
      export { MenuRegistry } from "./src/ui/menu/menu-registry.ts";
      export { MenuRenderer } from "./src/ui/menu/menu-renderer.ts";
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
  loader: { ".css": "empty" },
});
const bundlePath = path.join(os.tmpdir(), "menu-registries-test.mjs");
await writeFile(bundlePath, bundle.outputFiles[0].text);
const { CommandRegistry, MenuRegistry, MenuRenderer } = await import(pathToFileURL(bundlePath).href);

const failures = [];
function check(name, condition) {
  if (!condition) failures.push(name);
}

// A fresh jsdom per renderer scenario: the renderer reads the globals and
// attaches listeners to the DOM it finds at construction time.
function scenario() {
  const dom = new JSDOM(html, { url: "http://127.0.0.1:7910/" });
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.Element = dom.window.Element;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Node = dom.window.Node;
  return dom.window;
}

// --- Command registry -----------------------------------------------------------

{
  const commands = new CommandRegistry();
  let runs = 0;
  const registration = commands.register("test.run", { label: "Run", run: () => (runs += 1) });
  check("a registered command is found by lookup", commands.lookup("test.run")?.label === "Run");
  check("an unknown command is not found", commands.lookup("test.nope") === undefined);
  check("execute runs the command and reports it", commands.execute("test.run") === true && runs === 1);
  check("execute reports an unknown command as not run", commands.execute("test.nope") === false);
  commands.register("test.run", { label: "Run Again", run: () => (runs += 10) });
  commands.execute("test.run");
  check("re-registering upserts the descriptor", runs === 11 && commands.lookup("test.run").label === "Run Again");
  registration.dispose();
  commands.execute("test.run");
  check("disposing the original registration leaves the upsert in place", runs === 21);
}

// --- Menu registry ----------------------------------------------------------------

{
  const menus = new MenuRegistry();
  menus.registerMenu("edit", "Edit", 2);
  menus.registerMenu("file", "File", 1);
  check(
    "menus come out in registration order by position",
    menus.menusInOrder().map((menu) => menu.id).join(",") === "file,edit",
  );
  menus.setMenuItem("file", "file.a", { kind: "command", command: "a" });
  menus.setMenuItem("file", "file.b", { kind: "command", command: "b" });
  menus.setMenuItem("file", "file.sep", { kind: "separator" });
  menus.setMenuItem("file", "file.c", { kind: "command", command: "c" });
  menus.setMenuItem("file", "file.b", { kind: "command", command: "b2" });
  check(
    "an item upsert keeps its position",
    menus.itemSpecs("file").map((item) => item.id).join(",") === "file.a,file.b,file.sep,file.c" &&
      menus.itemSpecs("file")[1].spec.command === "b2",
  );
  const provider = { items: () => [] };
  menus.setProvider("model", provider);
  check("the provider round-trips", menus.providerFor("model") === provider);
  check("a menu without a provider has none", menus.providerFor("file") === undefined);
}

// --- Renderer: static menus ---------------------------------------------------------

{
  const window = scenario();
  const commands = new CommandRegistry();
  const menus = new MenuRegistry();
  let newAgentRuns = 0;
  let closeRuns = 0;
  commands.register("file.newAgent", { label: "New Agent", run: () => (newAgentRuns += 1) });
  commands.register("file.close", {
    label: "Close Window",
    shortcut: "Alt+F4",
    run: () => (closeRuns += 1),
  });
  menus.registerMenu("file", "File", 1);
  menus.setMenuItem("file", "file.newAgent", { kind: "command", command: "file.newAgent" });
  menus.setMenuItem("file", "file.sep", { kind: "separator" });
  menus.setMenuItem("file", "file.close", { kind: "command", command: "file.close" });

  const nav = window.document.querySelector(".ws-window-titlebar__menus");
  const renderer = new MenuRenderer(nav, commands, menus);
  const button = window.document.querySelector('[data-menu="file"]');
  const popover = button.nextElementSibling;
  check("the renderer builds a popover per registered menu", popover?.classList.contains("ws-window-titlebar__popover"));
  check("the popover starts hidden", popover.hidden === true);

  button.click();
  check("clicking the button opens the menu", popover.hidden === false);
  const rows = [...popover.querySelectorAll(".ws-window-titlebar__item")];
  check(
    "the rows render in placement order",
    rows.map((row) => row.querySelector(".ws-window-titlebar__item-label").textContent).join(",") ===
      "New Agent,Close Window",
  );
  check(
    "the separator renders between the rows",
    popover.querySelectorAll(".ws-window-titlebar__separator").length === 1,
  );
  check(
    "the shortcut hint renders",
    rows[1].querySelector(".ws-window-titlebar__shortcut")?.textContent === "Alt+F4",
  );

  rows[0].click();
  check("clicking a row runs its command", newAgentRuns === 1 && closeRuns === 0);
  check("activating a row closes the menu", popover.hidden === true);

  button.click();
  window.document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  check("Escape closes the open menu", popover.hidden === true);

  renderer.dispose();
  check(
    "disposal removes the popovers",
    window.document.querySelectorAll(".ws-window-titlebar__popover").length === 0,
  );
}

// --- Renderer: a provider menu rebuilds dynamically ----------------------------------

{
  const window = scenario();
  const commands = new CommandRegistry();
  const menus = new MenuRegistry();
  menus.registerMenu("model", "Model", 3);
  let models = ["alpha"];
  const listeners = new Set();
  menus.setProvider("model", {
    items: () =>
      models.map((id) => ({
        kind: "command",
        key: `model:${id}`,
        label: id,
        checked: id === "alpha",
        run: () => {},
      })),
    onDidChange: (listener) => {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    },
  });
  const nav = window.document.querySelector(".ws-window-titlebar__menus");
  const renderer = new MenuRenderer(nav, commands, menus);
  const button = window.document.querySelector('[data-menu="model"]');
  const popover = button.nextElementSibling;

  button.click();
  const labels = () =>
    [...popover.querySelectorAll(".ws-window-titlebar__item-label")].map((row) => row.textContent).join(",");
  check("the provider's rows render at open", labels() === "alpha");
  check(
    "a checked row renders as a checked radio",
    popover.querySelector(".ws-window-titlebar__item")?.getAttribute("aria-checked") === "true",
  );

  models = ["alpha", "beta"];
  for (const listener of [...listeners]) listener();
  check("a change while open rebuilds the rows", labels() === "alpha,beta");

  button.click(); // close
  models = ["gamma"];
  button.click(); // reopen
  check("the rows re-read the provider at open", labels() === "gamma");
  renderer.dispose();
}

// --- Renderer: a missing button is a loud error ----------------------------------------

{
  const window = scenario();
  const menus = new MenuRegistry();
  menus.registerMenu("bogus", "Bogus", 1);
  let threw = null;
  try {
    new MenuRenderer(window.document.querySelector(".ws-window-titlebar__menus"), new CommandRegistry(), menus);
  } catch (error) {
    threw = error;
  }
  check(
    "a menu without a title-bar button fails construction naming the menu",
    threw !== null && threw.message.includes("Bogus"),
  );
}

if (failures.length > 0) {
  console.error(`menu-registries: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("menu-registries: all assertions passed");
process.exit(0);
