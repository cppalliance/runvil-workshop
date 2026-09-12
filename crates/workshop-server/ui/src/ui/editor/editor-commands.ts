// The editor's workshop-level commands: save the active editor, close it
// (prompting on unsaved changes), and cycle the open editors. The
// directory's register() installs them into the command registry under
// the editor.* ids the shortcut bindings reference, so shortcuts.ts
// never imports the editor and CodeMirror stays out of the initial
// bundle. The commands resolve the dock through the service registry
// (the DOCK token, registered by initZones) instead of capturing it.

import type { IDockviewPanel } from "dockview";

import { DOCK, resolvePanelContent } from "../../services/panel-registry";
import { getService } from "../../services/service-registry";
import { EditorPanel } from "./editor-panel";

/** The panel's content as an EditorPanel, or null for other panel kinds. */
function asEditor(panel: IDockviewPanel | undefined): EditorPanel | null {
  if (panel === undefined) {
    return null;
  }
  // view.content may be the lazy wrapper while the chunk loads; unwrap
  // to the real panel before the instanceof check.
  const content = resolvePanelContent(panel.view.content);
  return content instanceof EditorPanel ? content : null;
}

/** Every open editor panel, in dock order. */
function editorPanels(): IDockviewPanel[] {
  return getService(DOCK).panels.filter((panel) => asEditor(panel) !== null);
}

/** Ctrl+S: save the active editor. A no-op when no editor is active. */
export function saveActiveEditor(): void {
  const editor = asEditor(getService(DOCK).activePanel);
  if (editor !== null) {
    // save() handles its own failures (error bar, conflict dialog).
    void editor.save();
  }
}

/** Ctrl+W: close the active editor, prompting on unsaved changes. */
export function closeActiveEditor(): void {
  asEditor(getService(DOCK).activePanel)?.requestClose();
}

/** Ctrl+Tab / Ctrl+Shift+Tab: cycle the open editors, wrapping around. */
export function cycleEditor(direction: 1 | -1): void {
  const dock = getService(DOCK);
  const editors = editorPanels();
  if (editors.length === 0) {
    return;
  }
  const current = editors.findIndex((panel) => panel === dock.activePanel);
  const index =
    current === -1
      ? direction === 1
        ? 0
        : editors.length - 1
      : (current + direction + editors.length) % editors.length;
  const panel = editors[index];
  if (panel === undefined) {
    return;
  }
  panel.api.setActive();
  asEditor(panel)?.focus();
}
