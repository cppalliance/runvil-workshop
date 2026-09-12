// chrome feature directory barrel: re-exports the directory's public API.
export * from "./about-dialog";
export * from "./model-picker-trigger";
export * from "./token-ring";
export * from "./update-view";
export * from "./window-chrome";
export * from "./zoom";

import { DisposableStore, type IDisposable } from "../../base/lifecycle";
import { registerCommand } from "../menu/command-registry";
import { resetZoom, zoomIn, zoomOut } from "./zoom";

/**
 * The chrome directory's registration: the zoom commands the shortcut
 * bindings dispatch to. Chrome loads eagerly, so the composition root
 * calls this at boot and collects the disposable into the root store
 * (lazy directories are registered by the panel registry instead).
 */
export function register(): IDisposable {
  const store = new DisposableStore();
  store.add(registerCommand("chrome.zoomIn", { run: zoomIn }));
  store.add(registerCommand("chrome.zoomOut", { run: zoomOut }));
  store.add(registerCommand("chrome.resetZoom", { run: resetZoom }));
  return store;
}
