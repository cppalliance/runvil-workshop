// The Gateway Config panel: a Dockview panel embedding the gateway's
// config SPA in an iframe served on the workshop server's dedicated
// loopback config origin (`GET /gateway/config-origin`). Running the
// config SPA on its own loopback origin gives it a separate DOM/realm and
// storage boundary from the workshop page while keeping the iframe
// sandboxed to scripts + its own origin. Traffic between the iframe and
// the workshop flows only through the window-level bridge in
// gateway-config-bridge.ts, pinned to both the resolved config origin
// and the live iframe window. The workshop's own origin is passed in the
// iframe URL's `bridge` parameter so the iframe pins its targetOrigin.

import "./gateway-config-panel.css";

import { getServiceOrNull } from "@workshop/platform/service-registry";
import { WorkshopPart } from "@workshop/platform/workshop-part";
import {
  fetchConfigOrigin,
  GATEWAY_CONFIG_FRAME_BRIDGE,
  parseLoopbackOrigin,
  type FetchLike,
  type FrameSource,
  type GatewayConfigFrameBridge,
} from "../../services/gateway-config-api";

/** Injectable seams for tests. */
export interface GatewayConfigPanelDeps {
  /** The workshop's own origin; window.location.origin in production. */
  readonly workshopOrigin?: string;
  /** Optional explicit config origin override for tests. */
  readonly configOrigin?: string;
  /** Optional startup gate awaited before resolving the origin and navigating the iframe. */
  readonly beforeLoad?: Promise<void>;
  /** Optional fetch implementation for `/gateway/config-origin`. */
  readonly fetchFn?: FetchLike;
  /** Optional config-frame bridge; defaults to the registered service. */
  readonly frameBridge?: GatewayConfigFrameBridge | null;
}

export class GatewayConfigPanel extends WorkshopPart {
  private panelDisposed = false;
  private frameSource: FrameSource | null = null;
  private iframe: HTMLIFrameElement | null = null;

  constructor(private readonly deps: GatewayConfigPanelDeps = {}) {
    super();
    this.element.className = "ws-gateway-config-panel";
  }

  protected create(parent: HTMLElement): void {
    void this.loadFrame(parent);
  }

  private async loadFrame(parent: HTMLElement): Promise<void> {
    const bridge =
      this.deps.frameBridge !== undefined ? this.deps.frameBridge : getServiceOrNull(GATEWAY_CONFIG_FRAME_BRIDGE);
    const startupGate = this.deps.beforeLoad ?? bridge?.startupReady;
    if (startupGate !== undefined) {
      await startupGate;
    }
    if (this.panelDisposed) {
      return;
    }
    const workshopOrigin = this.deps.workshopOrigin ?? window.location.origin;
    const rawOrigin =
      this.deps.configOrigin ?? (await fetchConfigOrigin(this.deps.fetchFn));
    if (this.panelDisposed || rawOrigin === null) {
      return;
    }
    const configOrigin = parseLoopbackOrigin(rawOrigin);
    if (configOrigin === null || configOrigin === workshopOrigin) {
      return;
    }
    // Mount the iframe only after startup capability binding has settled and
    // configOrigin is verified, so no frame exists during the re-bind window.
    const iframe = document.createElement("iframe");
    iframe.className = "ws-gateway-config-panel__frame";
    iframe.title = "Gateway Config";
    // allow-scripts runs the SPA; allow-same-origin keeps the iframe on
    // its dedicated loopback config origin instead of collapsing it to an
    // opaque null origin. allow-top-navigation is deliberately omitted so
    // the frame cannot navigate the top window onto the config origin.
    iframe.setAttribute("sandbox", "allow-scripts allow-same-origin");
    this.iframe = iframe;
    parent.replaceChildren(iframe);
    const frameSource: FrameSource = () => iframe.contentWindow;
    this.frameSource = frameSource;
    bridge?.attachFrame(configOrigin, frameSource);
    iframe.src = `${configOrigin}/?mode=panel&bridge=${encodeURIComponent(workshopOrigin)}`;
  }

  override dispose(): void {
    this.panelDisposed = true;
    const bridge =
      this.deps.frameBridge !== undefined ? this.deps.frameBridge : getServiceOrNull(GATEWAY_CONFIG_FRAME_BRIDGE);
    bridge?.detachFrame(this.frameSource);
    this.frameSource = null;
    if (this.iframe !== null) {
      this.iframe.removeAttribute("src");
      this.iframe = null;
    }
    this.element.replaceChildren();
    super.dispose();
  }
}
