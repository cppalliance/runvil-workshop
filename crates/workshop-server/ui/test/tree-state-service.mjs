// Unit test for the tree-state service (src/services/tree-state-service.ts):
// the Workshop tree's session state - expanded directory paths and the
// fetched listing cache - moved out of workshop-panel.ts module scope into
// a registry-held service, so a reopened panel restores the tree as the
// user left it without module-scope Maps. Bundles the module with esbuild
// and drives it. Covers: self-registration through the TREE_STATE token,
// the singleton surviving across lookups (the property a panel reopen
// relies on), expansion tracking, the listing cache, and root
// invalidation dropping only the synthetic roots listing.
// Run: node test/tree-state-service.mjs
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const uiDir = path.dirname(fileURLToPath(import.meta.url));

const bundle = await esbuild.build({
  stdin: {
    contents: `
      export { TreeStateService, TREE_STATE } from "./src/services/tree-state-service.ts";
      export { getService } from "./src/services/service-registry.ts";
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
});
const { TreeStateService, TREE_STATE, getService } = await import(
  `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
);

const failures = [];
function check(name, condition) {
  if (!condition) failures.push(name);
}

// --- Self-registration and the singleton ------------------------------------

const service = getService(TREE_STATE);
check("the TREE_STATE token self-registers", service instanceof TreeStateService);
check("the registry caches one instance", getService(TREE_STATE) === service);

// --- Expansion tracking ---------------------------------------------------------

const dir = "C:\\project\\src";
check("a directory starts collapsed", !service.isExpanded(dir));
service.expand(dir);
check("expand marks the directory expanded", service.isExpanded(dir));
service.collapse(dir);
check("collapse marks the directory collapsed", !service.isExpanded(dir));

// --- The listing cache -------------------------------------------------------------

const listing = { path: dir, entries: [] };
check("an uncached path has no listing", service.listing(dir) === undefined);
service.cacheListing(dir, listing);
check("the listing round-trips", service.listing(dir) === listing);

// The roots listing lives under the synthetic empty-path key; a workspace
// change invalidates it without dropping the directory listings.
service.cacheListing("", { path: null, entries: [] });
check("the roots listing caches under the empty key", service.listing("") !== undefined);
service.invalidateRoots();
check("invalidateRoots drops the roots listing", service.listing("") === undefined);
check("invalidateRoots keeps the directory listings", service.listing(dir) === listing);

service.dispose();

if (failures.length > 0) {
  console.error(`tree-state-service: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("tree-state-service: all assertions passed");
process.exit(0);
