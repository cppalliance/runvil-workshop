// App-level keyboard shortcuts: one document keydown listener dispatching
// key chords to commands in the command registry. The binding table below
// maps chords to command ids; the commands themselves are registered by
// the feature directories that own them (the editor's save and close, the
// tree's toggle and focus, chrome's zoom), so this module imports no
// feature code and the heavy panels stay out of the initial bundle. A
// matched chord always preventDefaults - even when its command is not
// registered yet (the feature chunk is still loading) - so the browser
// never sees Ctrl+W or Ctrl+S. CodeMirror keeps typing, selection,
// clipboard, undo/redo, and in-file find/replace; these bindings cover
// workspace-level actions only.

import { toDisposable, type IDisposable } from "../../base/lifecycle";
import { executeCommand } from "../menu/command-registry";

/**
 * One key chord bound to a command id. `matches` sees the raw keydown
 * after the plain-Ctrl gate; it must be specific enough to never collide
 * with another binding, because the first match wins.
 */
export interface Keybinding {
  readonly command: string;
  readonly matches: (event: KeyboardEvent) => boolean;
}

const keybindings: Keybinding[] = [];

/**
 * Binds a chord to a command id. Feature directories call this from
 * their register() functions, alongside their commands. The returned
 * disposable unbinds.
 */
export function registerKeybinding(keybinding: Keybinding): IDisposable {
  keybindings.push(keybinding);
  return toDisposable(() => {
    const index = keybindings.indexOf(keybinding);
    if (index !== -1) {
      keybindings.splice(index, 1);
    }
  });
}

/** Only plain Ctrl combinations are bound; Alt and Meta stay untouched. */
function isPlainCtrl(event: KeyboardEvent): boolean {
  return event.ctrlKey && !event.altKey && !event.metaKey;
}

// The built-in bindings. The commands behind them self-register:
// editor.* when the first editor panel's chunk loads, workshop.* when
// the tree's chunk loads (both happen at boot for the default layout),
// chrome.* from the eager chrome directory.
registerKeybinding({
  command: "editor.cyclePrevious",
  matches: (event) => event.key === "Tab" && event.shiftKey,
});
registerKeybinding({
  command: "editor.cycleNext",
  matches: (event) => event.key === "Tab" && !event.shiftKey,
});
// Zoom binds run without a Shift gate: Ctrl+Shift+= reports "+" for the
// same physical key Ctrl+= reports "=" for, and both are the conventional
// zoom-in chord.
registerKeybinding({
  command: "chrome.zoomIn",
  matches: (event) => event.code === "Equal" || event.key === "=" || event.key === "+",
});
registerKeybinding({
  command: "chrome.zoomOut",
  matches: (event) => event.code === "Minus" || event.key === "-",
});
registerKeybinding({
  command: "chrome.resetZoom",
  matches: (event) => event.code === "Digit0" || event.key === "0",
});
registerKeybinding({
  command: "workshop.focusTree",
  matches: (event) => event.shiftKey && event.key.toLowerCase() === "f",
});
registerKeybinding({
  command: "editor.save",
  matches: (event) => !event.shiftKey && event.key.toLowerCase() === "s",
});
registerKeybinding({
  command: "editor.close",
  matches: (event) => !event.shiftKey && event.key.toLowerCase() === "w",
});
registerKeybinding({
  command: "workshop.togglePanel",
  matches: (event) => !event.shiftKey && event.key.toLowerCase() === "b",
});

/**
 * Installs the app-level keydown listener. A matched chord dispatches its
 * command through the command registry; unbound combinations fall through
 * without preventDefault so the browser and CodeMirror keep theirs.
 * Returns the disposable that uninstalls it.
 */
export function installShortcuts(): IDisposable {
  const onKeydown = (event: KeyboardEvent): void => {
    if (!isPlainCtrl(event)) {
      return;
    }
    for (const keybinding of keybindings) {
      if (keybinding.matches(event)) {
        // Prevent the browser default even when the command has not
        // registered yet: Ctrl+W closing the browser tab because the
        // editor chunk was still loading would be a data-loss-shaped bug.
        event.preventDefault();
        executeCommand(keybinding.command);
        return;
      }
    }
  };
  document.addEventListener("keydown", onKeydown);
  return toDisposable(() => {
    document.removeEventListener("keydown", onKeydown);
  });
}
