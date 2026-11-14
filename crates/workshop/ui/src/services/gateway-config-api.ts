// The workshop server's gateway-config surface: the narrow server-side
// proxy (/gateway/api/{path}) that forwards the config UI's API calls
// with the gateway bearer key attached. The key lives only in the
// workshop server's process; neither the workshop page nor the iframe
// ever sees it.

/** The fetch signature the helpers use; tests substitute a stub. */
export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

import { createServiceToken, type ServiceToken } from "@workshop/platform/service-registry";

/** A live window source or lazy getter resolving the active config iframe window. */
export type FrameSource = MessageEventSource | (() => MessageEventSource | null);

/** Parent-window wiring shared between the composition root's bridge and the lazy config panel. */
export interface GatewayConfigFrameBridge {
  /** Resolves once the startup capability bind attempt has settled. */
  readonly startupReady: Promise<void>;
  /** Binds the active config iframe's origin and source window for postMessage filtering. */
  attachFrame(origin: string, frameSource: FrameSource | null): void;
  /** Clears the active config iframe registration when the panel disposes. */
  detachFrame(frameSource: FrameSource | null): void;
}

/** Registry token for the composition root's config-frame bridge. */
export const GATEWAY_CONFIG_FRAME_BRIDGE: ServiceToken<GatewayConfigFrameBridge> =
  createServiceToken<GatewayConfigFrameBridge>("workshop.gatewayConfigFrameBridge");

/**
 * Validates that `raw` is a bare `http:` loopback origin (`127.0.0.1`,
 * `localhost`, or `[::1]`) with no path, query, fragment, or credentials.
 */
export function parseLoopbackOrigin(raw: string): string | null {
  try {
    const parsed = new URL(raw);
    const hostname = parsed.hostname;
    const isLoopback =
      hostname === "127.0.0.1" || hostname === "localhost" || hostname === "::1" || hostname === "[::1]";
    if (
      parsed.protocol !== "http:" ||
      !isLoopback ||
      parsed.pathname !== "/" ||
      parsed.search !== "" ||
      parsed.hash !== "" ||
      parsed.username !== "" ||
      parsed.password !== ""
    ) {
      return null;
    }
    return parsed.origin;
  } catch {
    return null;
  }
}

/**
 * Fetches the dedicated config listener's loopback origin from the
 * workshop server (`GET /gateway/config-origin`). Returns `null` when
 * unavailable or non-loopback.
 */
export async function fetchConfigOrigin(fetchFn: FetchLike = defaultFetch): Promise<string | null> {
  try {
    const response = await fetchFn("/gateway/config-origin");
    if (!response.ok) {
      return null;
    }
    const payload = (await response.json()) as { origin?: unknown } | null;
    if (payload === null || typeof payload !== "object" || typeof payload.origin !== "string") {
      return null;
    }
    return parseLoopbackOrigin(payload.origin);
  } catch {
    return null;
  }
}

/** One API-forward request, as the config panel posts it over the bridge. */
export interface BridgeApiRequest {
  /** Correlation id; echoed on the result message. */
  readonly id: string;
  /** The HTTP method to forward. */
  readonly method: string;
  /** The gateway path (plus query), always starting with "/". */
  readonly path: string;
  /** The JSON request body, or null for a bodyless request. */
  readonly body: string | null;
}

/** The proxy's answer, relayed back to the iframe over the bridge. */
export interface BridgeApiResult {
  /** The gateway's status; 0 means the workshop server was unreachable. */
  readonly status: number;
  /** The gateway's Content-Type, or null when it sent none. */
  readonly contentType: string | null;
  /** The gateway's response body as text. */
  readonly body: string;
}

const defaultFetch: FetchLike = (input, init) => fetch(input, init);

/**
 * Forwards one bridged API request through the workshop server's
 * /gateway/api proxy, which attaches the bearer key and applies the
 * path allowlist. A transport failure answers status 0, so the iframe's
 * bridge client can surface it as an unreachable gateway rather than
 * hang.
 */
export async function forwardGatewayRequest(
  request: BridgeApiRequest,
  fetchFn: FetchLike = defaultFetch,
): Promise<BridgeApiResult> {
  if (!request.path.startsWith("/")) {
    // Absolute or malformed targets never leave the browser; the server
    // would refuse them anyway, but refusing here keeps the failure local.
    return { status: 403, contentType: null, body: "" };
  }
  const method = request.method.toUpperCase();
  const init: RequestInit = { method };
  if (["POST", "PUT", "PATCH"].includes(method)) {
    // The workshop server's cross-site guard requires body-bearing
    // methods to declare JSON - the bodyless POSTs of the config
    // surface (config-apply, config-revert) included.
    init.headers = { "Content-Type": "application/json" };
  }
  if (request.body !== null) {
    init.body = request.body;
  }
  try {
    const response = await fetchFn(`/gateway/api${request.path}`, init);
    return {
      status: response.status,
      contentType: response.headers.get("content-type"),
      body: await response.text(),
    };
  } catch {
    return { status: 0, contentType: null, body: "" };
  }
}
