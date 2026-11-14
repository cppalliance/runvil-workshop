// The workshop side of the config panel's postMessage bridge. The
// gateway config UI runs in a cross-origin iframe served by the
// workshop server's dedicated loopback config listener (`GET /gateway/config-origin`);
// this window-level listener is its only path to the workshop:
// API-forward requests go through the workshop server's key-attaching
// proxy, action notifications land on the status bar, and the ready
// announcement is answered with a context message (theme and initial
// route). Both origin and source window are pinned: a message is handled
// only when event.origin equals the config listener's loopback origin
// and event.source matches the active config iframe's contentWindow, and
// every reply is posted with that exact config origin as targetOrigin - never "*".

import { toDisposable, type IDisposable } from "@workshop/platform/lifecycle";
import {
  forwardGatewayRequest,
  parseLoopbackOrigin,
  type FetchLike,
  type FrameSource,
  type GatewayConfigFrameBridge,
} from "../../services/gateway-config-api";

/** The status surface the panel's action notifications land on. */
export interface BridgeStatusSink {
  /** Shows a local (client-originated) status line. */
  showLocal(label: string, severity: "info" | "error"): void;
}

/** Construction seams for {@link setupGatewayConfigBridge}. */
export interface GatewayConfigBridgeOptions {
  /** Where action notifications (apply, revert, download-started) land. */
  readonly statusBar: BridgeStatusSink;
  /** Transport for the API-forward proxy; the global fetch in production. */
  readonly fetchFn?: FetchLike;
  /** The window whose message events feed the bridge; the global one in production. */
  readonly win?: Pick<Window, "addEventListener" | "removeEventListener">;
  /**
   * Reply seam: posts `message` back to the event's source window at
   * `targetOrigin`. Tests substitute a recorder; production posts to
   * event.source with the pinned config origin.
   */
  readonly reply?: (event: MessageEvent, message: unknown, targetOrigin: string) => void;
  /** Resolves once trusted startup work (such as capability binding) has settled. */
  readonly startupReady?: Promise<void>;
  /** Initial allowed config origin for standalone tests; production registers dynamically via attachFrame. */
  readonly configOrigin?: string;
  /** Initial active frame source for standalone tests. */
  readonly frameSource?: FrameSource;
  /** Workshop's main origin; defaults to `window.location.origin`. */
  readonly workshopOrigin?: string;
}

/** Bridge handle exposing both disposal and active config-frame registration. */
export interface GatewayConfigBridgeHandle extends IDisposable, GatewayConfigFrameBridge {}

/** Status bar lines for the actions the config UI announces. */
const ACTION_LABELS: Readonly<Record<string, string>> = {
  apply: "Gateway configuration applied",
  revert: "Gateway configuration changes reverted",
  "download-started": "Gateway download started",
};

/** The context handed to the iframe once it announces itself. */
const PANEL_CONTEXT = { type: "rv-context", theme: "dark", route: "#/local" } as const;

function resolveFrameSource(source: FrameSource | null): MessageEventSource | null {
  if (source === null) {
    return null;
  }
  return typeof source === "function" ? source() : source;
}

/**
 * Installs the window-level message listener that serves the config
 * panel's iframe. Messages are accepted only from the registered config
 * frame window on the dedicated loopback config origin, and replies are
 * pinned to that exact config origin. The returned dispose() detaches
 * the listener and drops the active frame registration.
 */
export function setupGatewayConfigBridge(options: GatewayConfigBridgeOptions): GatewayConfigBridgeHandle {
  const win = options.win ?? window;
  const workshopOrigin =
    options.workshopOrigin ?? (typeof window !== "undefined" ? window.location.origin : "");
  const startupReady = options.startupReady ?? Promise.resolve();
  const reply =
    options.reply ??
    ((event: MessageEvent, message: unknown, targetOrigin: string): void => {
      (event.source as Window | null)?.postMessage(message, targetOrigin);
    });

  let disposed = false;
  let allowedOrigin: string | null = null;
  let activeFrameSource: FrameSource | null = null;

  const attachFrame = (origin: string, frameSource: FrameSource | null): void => {
    const parsed = parseLoopbackOrigin(origin);
    if (disposed || parsed === null || parsed === workshopOrigin || frameSource === null) {
      allowedOrigin = null;
      activeFrameSource = null;
      return;
    }
    allowedOrigin = parsed;
    activeFrameSource = frameSource;
  };

  const detachFrame = (frameSource: FrameSource | null): void => {
    if (frameSource === null || activeFrameSource === frameSource) {
      allowedOrigin = null;
      activeFrameSource = null;
    }
  };

  if (options.configOrigin !== undefined && options.frameSource !== undefined) {
    attachFrame(options.configOrigin, options.frameSource);
  }

  const onMessage = (event: MessageEvent): void => {
    void (async () => {
      const expectedOrigin = allowedOrigin;
      const expectedSource = resolveFrameSource(activeFrameSource);
      if (
        disposed ||
        expectedOrigin === null ||
        expectedSource === null ||
        event.origin !== expectedOrigin ||
        event.source !== expectedSource
      ) {
        return; // Not the active config panel; every foreign origin or spoofed window is dropped.
      }
      const data = event.data as Record<string, unknown> | null;
      if (data === null || typeof data !== "object") {
        return;
      }
      switch (data["type"]) {
        case "rv-bridge-ready": {
          reply(event, PANEL_CONTEXT, expectedOrigin);
          return;
        }
        case "rv-api": {
          const id = data["id"];
          const method = data["method"];
          const path = data["path"];
          const body = data["body"];
          if (typeof id !== "string" || typeof method !== "string" || typeof path !== "string") {
            return; // A malformed request has no id to answer under.
          }
          const result = await forwardGatewayRequest(
            { id, method, path, body: typeof body === "string" ? body : null },
            options.fetchFn,
          );
          if (
            disposed ||
            allowedOrigin !== expectedOrigin ||
            resolveFrameSource(activeFrameSource) !== expectedSource
          ) {
            return; // Panel was disposed or replaced while the request was in flight.
          }
          reply(event, { type: "rv-api-result", id, ...result }, expectedOrigin);
          return;
        }
        case "rv-action": {
          const action = data["action"];
          const label = typeof action === "string" ? ACTION_LABELS[action] : undefined;
          if (label !== undefined) {
            options.statusBar.showLocal(label, "info");
          }
          return;
        }
        default:
          return; // Unknown message kinds are dropped, never answered.
      }
    })();
  };

  win.addEventListener("message", onMessage);
  const listener = toDisposable(() => {
    disposed = true;
    allowedOrigin = null;
    activeFrameSource = null;
    win.removeEventListener("message", onMessage);
  });

  return {
    startupReady,
    attachFrame,
    detachFrame,
    dispose: () => listener.dispose(),
  };
}
