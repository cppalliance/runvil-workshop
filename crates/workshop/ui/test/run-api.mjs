// Unit test for the Run window's contract client (src/services/run-api.ts).
// Bundles the module with esbuild, imports it via a data URL, and drives it
// against a scripted fetch. Covers: a full contract narrows field by field
// (snake_case wire keys to the client's camelCase) and carries no tools
// section; the args schema narrows node by node, through properties and
// items; the default declaration; narrowing rejects an unknown type word and
// a root that is not an object; a 422 envelope surfaces the server's code and
// message; a non-JSON answer and a transport failure throw their typed
// variants.
// Run: node --test test/run-api.mjs
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const uiDir = path.dirname(fileURLToPath(import.meta.url));

const bundle = await esbuild.build({
  entryPoints: [path.join(uiDir, "..", "src", "services", "run-api.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "browser",
  target: "es2022",
  logLevel: "silent",
});
const { fetchPromptContract } = await import(
  `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
);

const failures = [];
function check(name, condition) {
  if (!condition) failures.push(name);
}

// The full-frontmatter contract, as POST /prompts/contract answers it.
const CONTRACT = {
  name: "papergate",
  description: "Screens papers",
  runvil: 1,
  max_tool_iterations: 12,
  uses: [{ id: "tools/web" }],
  args: {
    type: "object",
    properties: {
      deep: { type: "boolean", default: false },
      focus: { type: "array", items: { type: "string" }, default: ["methods"] },
      limit: { type: "integer", default: 5 },
      opts: {
        type: "object",
        properties: { depth: { type: "integer", enum: [1, 2] } },
        additionalProperties: true,
      },
      tone: { type: "string", enum: ["formal", "casual"] },
      topic: { type: "string", description: "the topic" },
    },
    required: ["topic"],
    additionalProperties: false,
  },
  models: [
    { label: "strong", min_context: 32000, thinking: true },
  ],
};

function scriptFetch(responder) {
  const calls = [];
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return responder(url, init);
  };
  return calls;
}

const json = (body, status = 200) => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => body,
});

// --- A full contract narrows field by field --------------------------------

{
  const calls = scriptFetch(() => json(CONTRACT));
  const contract = await fetchPromptContract("papergate.md", "prompt text");
  check(
    "the request posts name and text to the contract route",
    calls.length === 1 &&
      calls[0].url === "/prompts/contract" &&
      calls[0].init.method === "POST" &&
      JSON.parse(calls[0].init.body).name === "papergate.md" &&
      JSON.parse(calls[0].init.body).text === "prompt text",
  );
  check(
    "scalars narrow",
    contract.name === "papergate" &&
      contract.description === "Screens papers" &&
      contract.runvil === 1 &&
      contract.maxToolIterations === 12,
  );
  check(
    "uses narrow",
    contract.uses.length === 1 && contract.uses[0].id === "tools/web",
  );
  check("a contract without tools narrows with no tools section", !("tools" in contract));
  const { args } = contract;
  check(
    "the args root narrows as an object node with its required list",
    args.type === "object" &&
      args.additionalProperties === false &&
      args.required?.join(",") === "topic",
  );
  // The Run window draws one row per property, top to bottom, so the
  // narrowed properties must keep the order the server sends them.
  check(
    "the properties keep the order the server sends them",
    Object.keys(args.properties ?? {}).join(",") === "deep,focus,limit,opts,tone,topic",
  );
  check(
    "a property narrows with its type and default",
    args.properties.topic.type === "string" &&
      args.properties.topic.default === undefined &&
      args.properties.deep.type === "boolean" &&
      args.properties.deep.default === false &&
      args.properties.limit.type === "integer" &&
      args.properties.limit.default === 5,
  );
  // The Run window shows a described arg's text as a second line under its
  // label and draws no line for an undescribed one, so the narrowed
  // description must survive as the string, or as nothing when the prompt
  // gives none.
  check(
    "an arg's description narrows to its text, or to nothing when the prompt gives none",
    args.properties.topic.description === "the topic" &&
      args.properties.deep.description === undefined &&
      args.properties.limit.description === undefined,
  );
  check(
    "an enum narrows to its allowed values, strings or integers",
    args.properties.tone.enum?.join(",") === "formal,casual" &&
      args.properties.opts.properties.depth.enum?.join(",") === "1,2" &&
      args.properties.opts.properties.depth.enum?.[0] === 1,
  );
  check(
    "an array node narrows its items and keeps its default",
    args.properties.focus.type === "array" &&
      args.properties.focus.items?.type === "string" &&
      JSON.stringify(args.properties.focus.default) === '["methods"]',
  );
  check(
    "a nested object node narrows its own properties",
    args.properties.opts.type === "object" &&
      args.properties.opts.additionalProperties === true &&
      args.properties.opts.required === undefined &&
      args.properties.opts.properties.depth.type === "integer",
  );
  check(
    "model roles narrow",
    contract.models.length === 1 &&
      contract.models[0].label === "strong" &&
      contract.models[0].minContext === 32000 &&
      contract.models[0].thinking === true,
  );
}

// --- Absent sections narrow to null; the default declaration is one string --

{
  scriptFetch(() =>
    json({
      name: "bare",
      description: "",
      runvil: null,
      max_tool_iterations: null,
      uses: [],
      args: {
        type: "object",
        properties: {
          text: { type: "string", description: "Freeform text for this prompt" },
        },
        additionalProperties: false,
      },
      models: [],
    }),
  );
  const contract = await fetchPromptContract("bare.md", "text");
  check(
    "absent sections narrow to null",
    contract.runvil === null && contract.maxToolIterations === null,
  );
  check(
    "the default declaration narrows to one optional string named text",
    Object.keys(contract.args.properties ?? {}).join(",") === "text" &&
      contract.args.properties.text.type === "string" &&
      contract.args.properties.text.description === "Freeform text for this prompt" &&
      contract.args.required === undefined,
  );
}

// --- Model requirements: null forms narrow to null, a bad shape is refused ---

{
  scriptFetch(() =>
    json({
      ...CONTRACT,
      models: [{ label: "fast", min_context: null, thinking: null }],
    }),
  );
  const contract = await fetchPromptContract("p.md", "text");
  check(
    "a role with no requirements narrows both to null",
    contract.models.length === 1 &&
      contract.models[0].label === "fast" &&
      contract.models[0].minContext === null &&
      contract.models[0].thinking === null,
  );
}

{
  scriptFetch(() =>
    json({
      ...CONTRACT,
      models: [{ label: "strong", min_context: null, thinking: "yes" }],
    }),
  );
  let caught = null;
  try {
    await fetchPromptContract("p.md", "text");
  } catch (error) {
    caught = error;
  }
  check(
    "a model entry whose thinking is a string is a shape failure",
    caught !== null && caught.code === "unexpected_shape",
  );
}

// --- Narrowing rejects unknown variants -------------------------------------

{
  scriptFetch(() =>
    json({
      ...CONTRACT,
      args: {
        type: "object",
        properties: { when: { type: "date" } },
        additionalProperties: false,
      },
    }),
  );
  let caught = null;
  try {
    await fetchPromptContract("p.md", "text");
  } catch (error) {
    caught = error;
  }
  check(
    "an unknown type word is a shape failure",
    caught !== null && caught.code === "unexpected_shape",
  );
}

{
  scriptFetch(() =>
    json({
      ...CONTRACT,
      args: {
        type: "object",
        properties: { tags: { type: "array", items: { type: "date" } } },
        additionalProperties: false,
      },
    }),
  );
  let caught = null;
  try {
    await fetchPromptContract("p.md", "text");
  } catch (error) {
    caught = error;
  }
  check(
    "an unknown type word under items is a shape failure",
    caught !== null && caught.code === "unexpected_shape",
  );
}

{
  scriptFetch(() => json({ ...CONTRACT, args: { type: "string" } }));
  let caught = null;
  try {
    await fetchPromptContract("p.md", "text");
  } catch (error) {
    caught = error;
  }
  check(
    "a root that is not an object node is a shape failure",
    caught !== null && caught.code === "unexpected_shape",
  );
}

// --- A 422 envelope surfaces the server's code and message ------------------

{
  scriptFetch(() =>
    json(
      { error: { code: "parse_frontmatter", message: "line 3: bad YAML key" } },
      422,
    ),
  );
  let caught = null;
  try {
    await fetchPromptContract("broken.md", "text");
  } catch (error) {
    caught = error;
  }
  check(
    "a 422 surfaces the status",
    caught !== null && caught.status === 422,
  );
  check(
    "a 422 surfaces the server's code and message",
    caught !== null &&
      caught.message.includes("parse_frontmatter") &&
      caught.message.includes("line 3: bad YAML key"),
  );
}

// --- Transport and non-JSON failures throw their typed variants -------------

{
  scriptFetch(() => {
    throw new TypeError("connection refused");
  });
  let caught = null;
  try {
    await fetchPromptContract("p.md", "text");
  } catch (error) {
    caught = error;
  }
  check("a transport failure is typed", caught !== null && caught.code === "transport");
}

if (failures.length > 0) {
  console.error(`run-api: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("run-api: all assertions passed");
process.exit(0);
