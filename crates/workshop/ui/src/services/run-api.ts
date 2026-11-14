// Validated HTTP boundary for the prompt-contract API. POST
// /prompts/contract turns prompt markdown into the Run window's
// contract DTO; every response arrives as unknown and is parsed field
// by field into the narrow types below before any consumer touches it,
// no casts. Failures throw typed CatalogError variants
// (services/error-catalog.ts); a 422 parse failure surfaces the
// server's parse_<kind> code and line-numbered message in the thrown
// error's message. The fetch and JSON mechanics are the shared floor
// in json-request.ts.

import { CatalogError, ErrorCatalog } from "./error-catalog";
import { errorCode, errorMessage, isRecord, readJson, request } from "./json-request";

/** A declared Pack: its one-segment name, such as `web`. */
export interface RunContractPack {
  readonly id: string;
}

/** The words a schema node's `type` may hold. */
export type RunContractSchemaType =
  | "string"
  | "boolean"
  | "integer"
  | "number"
  | "array"
  | "object";

/**
 * One node of the `args` declaration, a subset of JSON Schema. The root
 * is an object node whose properties are the prompt's arguments.
 */
export interface RunContractSchema {
  readonly type: RunContractSchemaType;
  /** An object node's fields by name, in the order the server sends them. */
  readonly properties?: Readonly<Record<string, RunContractSchema>>;
  /** The names of the fields an object node requires. */
  readonly required?: readonly string[];
  /** Whether an object node takes fields it does not declare. */
  readonly additionalProperties?: boolean;
  /** The node of each item of an array node. */
  readonly items?: RunContractSchema;
  /** The values a string or integer node allows. */
  readonly enum?: readonly (string | number)[];
  /** The human-readable description. */
  readonly description?: string;
  /** The JSON value a field takes when the caller leaves it out. */
  readonly default?: unknown;
}

/** The requirements a prompt declares for one model role. */
export interface RunContractModel {
  /** The role name. */
  readonly label: string;
  /** The minimum context window in tokens; null when absent. */
  readonly minContext: number | null;
  /** The thinking requirement; null when absent. */
  readonly thinking: boolean | null;
}

/** The Run-window contract: everything the panel renders. */
export interface RunContract {
  readonly name: string;
  readonly description: string;
  /** The Engine major the `runvil:` key declares; null when absent. */
  readonly runvil: number | null;
  /** The declared tool-loop cap; null for the runtime default. */
  readonly maxToolIterations: number | null;
  readonly uses: readonly RunContractPack[];
  /** The `args` declaration: an object node. */
  readonly args: RunContractSchema;
  readonly models: readonly RunContractModel[];
}

function parsePack(value: unknown): RunContractPack | null {
  if (!isRecord(value)) {
    return null;
  }
  const { id } = value;
  if (typeof id !== "string") {
    return null;
  }
  return { id };
}

function parseSchemaType(value: unknown): RunContractSchemaType | null {
  switch (value) {
    case "string":
    case "boolean":
    case "integer":
    case "number":
    case "array":
    case "object":
      return value;
    default:
      return null;
  }
}

function parseProperties(value: unknown): Record<string, RunContractSchema> | null {
  if (!isRecord(value)) {
    return null;
  }
  const nodes: [string, RunContractSchema][] = [];
  for (const [name, field] of Object.entries(value)) {
    const node = parseSchema(field);
    if (node === null) {
      return null;
    }
    nodes.push([name, node]);
  }
  return Object.fromEntries(nodes);
}

function parseRequired(value: unknown): string[] | null {
  if (!Array.isArray(value)) {
    return null;
  }
  const names: string[] = [];
  for (const name of value) {
    if (typeof name !== "string") {
      return null;
    }
    names.push(name);
  }
  return names;
}

function parseEnum(value: unknown): (string | number)[] | null {
  if (!Array.isArray(value)) {
    return null;
  }
  const choices: (string | number)[] = [];
  for (const choice of value) {
    if (typeof choice !== "string" && typeof choice !== "number") {
      return null;
    }
    choices.push(choice);
  }
  return choices;
}

/** Narrows one schema node and, through `properties` and `items`, every node under it. */
function parseSchema(value: unknown): RunContractSchema | null {
  if (!isRecord(value)) {
    return null;
  }
  const type = parseSchemaType(value.type);
  const properties = value.properties === undefined ? undefined : parseProperties(value.properties);
  const required = value.required === undefined ? undefined : parseRequired(value.required);
  const items = value.items === undefined ? undefined : parseSchema(value.items);
  const choices = value.enum === undefined ? undefined : parseEnum(value.enum);
  if (
    type === null ||
    properties === null ||
    required === null ||
    items === null ||
    choices === null
  ) {
    return null;
  }
  const { additionalProperties, description } = value;
  if (additionalProperties !== undefined && typeof additionalProperties !== "boolean") {
    return null;
  }
  if (description !== undefined && typeof description !== "string") {
    return null;
  }
  return {
    type,
    properties,
    required,
    additionalProperties,
    items,
    enum: choices,
    description,
    default: value.default,
  };
}

function parseModel(value: unknown): RunContractModel | null {
  if (!isRecord(value)) {
    return null;
  }
  const { label, min_context, thinking } = value;
  if (typeof label !== "string") {
    return null;
  }
  if (min_context !== null && typeof min_context !== "number") {
    return null;
  }
  if (thinking !== null && typeof thinking !== "boolean") {
    return null;
  }
  return {
    label,
    minContext: min_context ?? null,
    thinking: thinking ?? null,
  };
}

function parseContract(body: unknown): RunContract | null {
  if (!isRecord(body)) {
    return null;
  }
  const { name, description, runvil, max_tool_iterations, uses, models } = body;
  if (typeof name !== "string" || typeof description !== "string") {
    return null;
  }
  if (runvil !== null && typeof runvil !== "number") {
    return null;
  }
  if (max_tool_iterations !== null && typeof max_tool_iterations !== "number") {
    return null;
  }
  if (!Array.isArray(uses) || !Array.isArray(models)) {
    return null;
  }
  const parsedPacks: RunContractPack[] = [];
  for (const pack of uses) {
    const parsed = parsePack(pack);
    if (parsed === null) {
      return null;
    }
    parsedPacks.push(parsed);
  }
  // The root of the declaration is an object node.
  const args = parseSchema(body.args);
  if (args === null || args.type !== "object") {
    return null;
  }
  const parsedModels: RunContractModel[] = [];
  for (const model of models) {
    const parsed = parseModel(model);
    if (parsed === null) {
      return null;
    }
    parsedModels.push(parsed);
  }
  return {
    name,
    description,
    runvil: runvil ?? null,
    maxToolIterations: max_tool_iterations ?? null,
    uses: parsedPacks,
    args,
    models: parsedModels,
  };
}

/**
 * Throws the typed failure for one non-OK response. A 422 parse failure
 * reports the server's parse_<kind> code ahead of its line-numbered
 * message, so the panel's error row shows both.
 */
function httpFailure(body: unknown, status: number, route: string): never {
  const message = errorMessage(body, status, route);
  const code = errorCode(body);
  throw new CatalogError(ErrorCatalog.HttpStatus, code === null ? message : `[${code}] ${message}`, {
    status,
  });
}

/**
 * Parses prompt markdown into the Run window's contract. `name` is the
 * prompt's display name (the file's, when it came from one). Throws
 * typed CatalogError variants on transport, HTTP, and shape failures.
 */
export async function fetchPromptContract(name: string, text: string): Promise<RunContract> {
  const route = "/prompts/contract";
  const response = await request(route, `POST ${route}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name, text }),
  });
  const body = await readJson(response, `POST ${route}`);
  if (!response.ok) {
    httpFailure(body, response.status, `POST ${route}`);
  }
  const contract = parseContract(body);
  if (contract === null) {
    throw new CatalogError(
      ErrorCatalog.UnexpectedShape,
      `POST ${route} returned an unexpected shape`,
    );
  }
  return contract;
}
