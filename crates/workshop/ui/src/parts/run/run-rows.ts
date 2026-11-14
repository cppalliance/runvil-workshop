// The Run window's contract rows: one renderer per contract section,
// pure DOM building over the narrowed RunContract. Every frontmatter
// key present in the prompt except `uses`, which has nothing to
// bind, appears as exactly one row: name, description, and runvil
// read-only; output a text field; one row for each top-level property
// of the `args` schema, its control chosen by the property's type, with
// the required marker and the default prefilled; model roles
// read-only; max_tool_iterations numeric with the runtime-default
// placeholder. No bind-time validation runs here - validation is the
// parser's alone.
//
// The rows are Cursor Settings rows: a text column holding the label (with
// the required marker beside it) and, for an arg that has one, its
// description as a visible second line, and a controls column on the other
// side. A boolean arg is a `.switch` button, as Cursor's settings toggles
// are.

import type { RunContract, RunContractSchema } from "../../services/run-api";

/** The Engine's name for an argument that holds free text. */
const TEXT_ARG = "text";

/** What a row shows beside its label. */
interface RowDetails {
  /** The second line under the label; blank or absent draws none. */
  readonly description?: string;
  /** Draws the required marker beside the label. */
  readonly required?: boolean;
}

/**
 * One Settings row: a text column (the label, its required marker, and the
 * optional description line) beside a controls column holding the given nodes.
 */
function row(label: string, controls: readonly Node[], details: RowDetails = {}): HTMLElement {
  const element = document.createElement("div");
  element.className = "ws-run-panel__row";

  const text = document.createElement("div");
  text.className = "ws-run-panel__row-text";
  const title = document.createElement("div");
  title.className = "ws-run-panel__row-title";
  const labelElement = document.createElement("span");
  labelElement.className = "ws-run-panel__row-label";
  labelElement.textContent = label;
  title.appendChild(labelElement);
  if (details.required === true) {
    title.appendChild(requiredMarker());
  }
  text.appendChild(title);
  if (details.description !== undefined && details.description.length > 0) {
    const description = document.createElement("div");
    description.className = "ws-run-panel__row-description";
    description.textContent = details.description;
    text.appendChild(description);
  }

  const controlColumn = document.createElement("div");
  controlColumn.className = "ws-run-panel__row-controls";
  controlColumn.append(...controls);
  element.append(text, controlColumn);
  return element;
}

/** A read-only value span. */
function readOnly(text: string): HTMLElement {
  const value = document.createElement("span");
  value.className = "ws-run-panel__value";
  value.textContent = text;
  return value;
}

/** A text field with the shared control skin. */
function textField(value: string, placeholder = ""): HTMLInputElement {
  const field = document.createElement("input");
  field.type = "text";
  field.className = "input ws-run-panel__field";
  field.value = value;
  field.placeholder = placeholder;
  return field;
}

/** A multiline box with the shared control skin. */
function multilineBox(value: string, placeholder = ""): HTMLTextAreaElement {
  const box = document.createElement("textarea");
  box.className = "input ws-run-panel__prose";
  box.value = value;
  box.placeholder = placeholder;
  return box;
}

/** The required marker on an arg the schema's `required` list names. */
function requiredMarker(): HTMLElement {
  const marker = document.createElement("span");
  marker.className = "ws-run-panel__required";
  marker.textContent = "*";
  marker.title = "required";
  return marker;
}

/**
 * A boolean arg's control: Cursor's switch, a `<button role="switch">` whose
 * `aria-checked` is the value. A click or Space/Enter flips it.
 */
function booleanSwitch(name: string, arg: RunContractSchema): HTMLButtonElement {
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "switch ws-run-panel__switch";
  toggle.setAttribute("role", "switch");
  toggle.setAttribute("aria-label", name);
  toggle.setAttribute("aria-checked", String(arg.default === true));
  toggle.addEventListener("click", () => {
    toggle.setAttribute("aria-checked", String(toggle.getAttribute("aria-checked") !== "true"));
  });
  return toggle;
}

/** The control of the arg `name` by its declared type, prefilled from its default. */
function argControl(name: string, arg: RunContractSchema): HTMLElement {
  if (arg.type === "boolean") {
    return booleanSwitch(name, arg);
  }
  if (arg.type === "integer" || arg.type === "number") {
    const field = document.createElement("input");
    field.type = "number";
    field.className = "input ws-run-panel__field";
    if (arg.type === "integer") {
      field.step = "1";
    }
    if (typeof arg.default === "number") {
      field.value = String(arg.default);
    }
    return field;
  }
  if (arg.type === "array" || arg.type === "object") {
    // A list or an object is typed as JSON text.
    return multilineBox(arg.default === undefined ? "" : JSON.stringify(arg.default), "JSON");
  }
  const value = typeof arg.default === "string" ? arg.default : "";
  // An `enum` shows its allowed values in the empty control.
  const placeholder = arg.enum?.join(" | ") ?? "";
  return name === TEXT_ARG ? multilineBox(value, placeholder) : textField(value, placeholder);
}

/**
 * Renders every contract section into `container`. The caller clears
 * the container first; rows append in contract order.
 */
export function renderContractRows(container: HTMLElement, contract: RunContract): void {
  const rows = document.createElement("div");
  rows.className = "ws-run-panel__rows";

  rows.appendChild(row("name", [readOnly(contract.name)]));
  if (contract.description.length > 0) {
    rows.appendChild(row("description", [readOnly(contract.description)]));
  }
  if (contract.runvil !== null) {
    rows.appendChild(row("runvil", [readOnly(String(contract.runvil))]));
  }

  rows.appendChild(
    row("output", [textField(contract.output?.path ?? "", contract.output?.description ?? "")]),
  );

  const required = contract.args.required ?? [];
  for (const [name, arg] of Object.entries(contract.args.properties ?? {})) {
    // The description is a visible second line under the label, not a tooltip.
    rows.appendChild(
      row(name, [argControl(name, arg)], {
        description: arg.description,
        required: required.includes(name),
      }),
    );
  }

  for (const model of contract.models) {
    const parts: string[] = [];
    if (model.minContext !== null) {
      parts.push(`min context ${model.minContext}`);
    }
    if (model.thinking !== null) {
      parts.push(model.thinking ? "thinking" : "no thinking");
    }
    const details = parts.join(", ");
    const modelRow = row(model.label, [readOnly(details)]);
    modelRow.classList.add("ws-run-panel__row--model");
    rows.appendChild(modelRow);
  }

  const iterations = document.createElement("input");
  iterations.type = "number";
  iterations.className = "input ws-run-panel__field";
  iterations.step = "1";
  if (contract.maxToolIterations === null) {
    iterations.placeholder = "runtime default";
  } else {
    iterations.value = String(contract.maxToolIterations);
  }
  rows.appendChild(row("max_tool_iterations", [iterations]));

  container.appendChild(rows);
}
