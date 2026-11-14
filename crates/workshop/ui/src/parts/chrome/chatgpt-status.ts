// The account item on the bottom status bar.
//
// The words live in a closed shadow root. The bar is the two words
// "ChatGPT plan". A ready plan puts the email on the tooltip inside that
// shadow, so a same-origin frame can see the outer span and cannot read the
// address, and a long address cannot stretch the bar. The popup is the
// one action for the state. Signed out, the slot is hidden.

import { Disposable, toDisposable } from "@workshop/platform/lifecycle";
import {
  OPENAI_CHATGPT_MARK_SVG,
  accountButtonLabel,
  type AccountButton,
  type ChatgptAccount,
} from "../../services/chatgpt-account";

/** Below MicMeter (-1) so showing or hiding the account item keeps the meter and LEDs pinned right. */
export const CHATGPT_STATUS_ORDER = -2;

export class ChatgptStatus extends Disposable {
  readonly element: HTMLElement;
  private readonly root: ShadowRoot;
  private readonly styleEl: HTMLStyleElement;
  private detachPopupListeners: (() => void) | null = null;
  private open = false;

  constructor(
    private readonly account: ChatgptAccount,
    slot?: HTMLElement,
  ) {
    super();
    // A button cannot hold a shadow root. The slot stays in the light DOM
    // and the words live on a span inside it.
    this.element = document.createElement("span");
    this.element.className = "ws-chatgpt-status";
    this.slot = slot ?? this.element;
    if (slot !== undefined) {
      slot.replaceChildren(this.element);
    }
    this.root = this.element.attachShadow({ mode: "closed" });
    this.styleEl = document.createElement("style");
    this.styleEl.textContent = SHADOW_CSS;
    this._register(account.onDidChange(() => this.render()));
    const onSlotClick = (): void => {
      if (this.account.chip.hidden) {
        return;
      }
      this.open = !this.open;
      this.render();
    };
    this.slot.addEventListener("click", onSlotClick);
    this._register(toDisposable(() => this.slot.removeEventListener("click", onSlotClick)));
    this._register(
      toDisposable(() => {
        this.open = false;
        this.clearPopupListeners();
        this.root.replaceChildren();
      }),
    );
    this.render();
  }

  private readonly slot: HTMLElement;

  private clearPopupListeners(): void {
    if (this.detachPopupListeners !== null) {
      this.detachPopupListeners();
      this.detachPopupListeners = null;
    }
  }

  private render(): void {
    this.clearPopupListeners();
    const chip = this.account.chip;
    this.slot.hidden = chip.hidden;
    this.root.replaceChildren();
    if (chip.hidden) {
      this.open = false;
      return;
    }
    const bar = document.createElement("span");
    bar.className = "ws-chatgpt-status__bar";
    if (chip.warn) {
      bar.dataset["warn"] = "true";
    }
    bar.title = chip.tooltip;
    // The mark is left out until OpenAI's permission is settled; see the comment on SIGN_IN_LABEL.
    if (OPENAI_CHATGPT_MARK_SVG !== null && chip.label === "ChatGPT plan") {
      const mark = document.createElement("span");
      mark.className = "ws-chatgpt-status__mark";
      mark.innerHTML = OPENAI_CHATGPT_MARK_SVG;
      bar.append(mark);
    }
    const label = document.createElement("span");
    label.className = "ws-chatgpt-status__label";
    label.textContent = chip.label;
    bar.append(label);
    this.root.append(this.styleEl, bar);
    if (!this.open || chip.button === null) {
      return;
    }
    const pop = document.createElement("div");
    pop.className = "ws-chatgpt-status__pop";
    pop.append(this.action(chip.button, chip.buttonDisabled));
    this.root.append(pop);
    this.place(pop);
    const onOutsidePointerDown = (event: PointerEvent): void => {
      if (!this.slot.contains(event.target as Node)) {
        this.open = false;
        this.render();
      }
    };
    const onEscape = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.preventDefault();
        this.open = false;
        this.render();
        if (this.slot.isConnected) {
          this.slot.focus();
        }
      }
    };
    document.addEventListener("pointerdown", onOutsidePointerDown);
    document.addEventListener("keydown", onEscape);
    this.detachPopupListeners = (): void => {
      document.removeEventListener("pointerdown", onOutsidePointerDown);
      document.removeEventListener("keydown", onEscape);
    };
  }

  /** Opens the popup just above the status item, inside the viewport. */
  private place(pop: HTMLElement): void {
    const item = this.slot.getBoundingClientRect();
    const margin = 6;
    const popBox = pop.getBoundingClientRect();
    const popHeight = popBox.height || pop.offsetHeight || 64;
    const popWidth = popBox.width || pop.offsetWidth || 180;
    let top = item.top - margin - popHeight;
    if (top < 8) {
      top = 8;
    }
    let left = item.right - popWidth;
    if (left < 8) {
      left = 8;
    }
    if (left + popWidth > window.innerWidth - 8) {
      left = Math.max(8, window.innerWidth - popWidth - 8);
    }
    pop.style.top = `${top}px`;
    pop.style.left = `${left}px`;
    pop.style.right = "auto";
    pop.style.bottom = "auto";
  }

  private action(button: AccountButton, disabled: boolean): HTMLButtonElement {
    const control = document.createElement("button");
    control.type = "button";
    control.className = "ws-chatgpt-status__item";
    control.textContent = accountButtonLabel(button);
    control.disabled = disabled;
    control.addEventListener("click", (event) => {
      event.stopPropagation();
      this.open = false;
      this.render();
      void this.run(button);
    });
    return control;
  }

  private async run(button: AccountButton): Promise<void> {
    if (button === "sign-out") {
      await this.account.signOut();
    } else if (button === "start-over" || button === "sign-in-again") {
      await this.account.signIn();
    } else if (button === "reset") {
      await this.account.reset();
    } else if (button === "try-again") {
      await this.account.enable();
    } else if (button === "check-again") {
      await this.account.refresh();
    } else {
      this.account.dismissCleanup();
    }
    this.render();
  }
}

// A closed shadow root does not inherit document.head stylesheets, so it owns an inline <style>; the popup matches @workshop/look's .menu-composer surface.
const SHADOW_CSS = `
:host { display: inline-flex; align-items: center; }
.ws-chatgpt-status__bar { display: inline-flex; align-items: center; gap: var(--space-1); font: inherit; font-weight: 400; color: inherit; white-space: nowrap; }
.ws-chatgpt-status__bar[data-warn="true"] { color: color-mix(in srgb, var(--cursor-yellow) 45%, currentColor); }
.ws-chatgpt-status__mark { display: inline-flex; }
.ws-chatgpt-status__pop { position: fixed; z-index: 9999; display: flex; flex-direction: column; min-inline-size: var(--ws-size-120); padding: var(--ws-size-2); background: var(--cursor-editor); color: var(--cursor-text-primary); border: var(--ws-border-width) solid var(--cursor-stroke-focused); border-radius: var(--radius); box-shadow: var(--shadow-popup); font: inherit; font-size: var(--font-size-sm); line-height: var(--line-height-sm); font-weight: 400; white-space: nowrap; }
.ws-chatgpt-status__item { display: flex; align-items: center; inline-size: 100%; min-block-size: var(--height-xs); padding: var(--ws-size-2) var(--space-1-5); font: inherit; font-size: var(--font-size-sm); line-height: var(--line-height-sm); font-weight: 400; color: var(--cursor-text-primary); background: transparent; border: none; border-radius: var(--radius-sm); text-align: start; cursor: pointer; }
.ws-chatgpt-status__item:hover:not(:disabled) { background: var(--bg-hover); }
.ws-chatgpt-status__item:focus-visible { outline: var(--focus-outline); outline-offset: var(--focus-outline-offset); background: var(--bg-hover); }
.ws-chatgpt-status__item:disabled { color: var(--cursor-control-disabled-text); cursor: default; }
`;
