// The model picker trigger: a toolbar button showing the selected
// model's id, with a 9px chevron. Clicking it (or Ctrl+/, through open)
// opens a DropdownMenu of the ModelService catalog on the composer's menu
// surface - check-only selection, 230px wide, at most 320px tall - and
// picking one sends the select command through the service. The label
// never updates optimistically - the server owns the selection, so the
// trigger re-renders only when the service's selection changes. The label
// shows the selected id alone, so a catalog change leaves it as it is; the
// menu reads the catalog each time it opens.

import "./model-picker-trigger.css";

import { ICON_CHEVRON_DOWN } from "@workshop/look/icons";
import { Disposable, toDisposable } from "@workshop/platform/lifecycle";
import { PLAN_TAG, SIGN_IN_LABEL, type ChatgptAccount } from "../../services/chatgpt-account";
import type { ModelService } from "../../services/model-service";
import { DropdownMenu, type DropdownEntry, type DropdownItem } from "@workshop/look/dropdown";

/** The trigger label when no model is selected. */
const NO_SELECTION_LABEL = "Select model";

/** The single menu row shown when the catalog is empty. */
const EMPTY_CATALOG_LABEL = SIGN_IN_LABEL;

/** The trigger's tooltip. */
const TRIGGER_TITLE = "Switch Model (Ctrl+/)";

/**
 * The trigger button plus its dropdown. Disposable: dispose() closes an
 * open menu, removes the click listener, and unsubscribes from the
 * service. The service is borrowed, not owned - the composition root
 * disposes it.
 */
export class ModelPickerTrigger extends Disposable {
  /** The trigger button; append it where the picker belongs. */
  readonly element: HTMLButtonElement;

  private readonly dropdown: DropdownMenu;
  private readonly labelSlot: HTMLSpanElement;

  constructor(
    private readonly modelService: ModelService,
    private readonly account?: ChatgptAccount,
  ) {
    super();

    this.element = document.createElement("button");
    this.element.type = "button";
    this.element.className = "ws-model-picker-trigger";
    this.element.title = TRIGGER_TITLE;

    this.labelSlot = document.createElement("span");
    this.labelSlot.className = "ws-model-picker-trigger__label";

    const iconSlot = document.createElement("span");
    iconSlot.className = "ws-model-picker-trigger__icon";
    iconSlot.setAttribute("aria-hidden", "true");
    // The chevron is a static string from @workshop/look, never input.
    iconSlot.innerHTML = ICON_CHEVRON_DOWN;

    this.element.append(this.labelSlot, iconSlot);

    this.dropdown = this._register(new DropdownMenu());

    const onClick = (): void => this.showMenu();
    this.element.addEventListener("click", onClick);
    this._register(
      toDisposable(() => this.element.removeEventListener("click", onClick)),
    );

    this._register(this.modelService.onDidChangeCurrent(() => this.renderCurrent()));

    this.renderCurrent();
  }

  /** Opens the model menu (Ctrl+/); a menu already open stays open. */
  open(): void {
    if (!this.dropdown.isOpen) {
      this.showMenu();
    }
  }

  private showMenu(): void {
    const models = this.modelService.models;
    const current = this.modelService.current;
    const planIds = new Set(this.account?.snapshot.planModels ?? []);
    const toItem = (id: string): DropdownItem => ({
      label: id,
      selected: id === current,
      onClick: () => this.select(id),
    });
    const otherItems: DropdownItem[] = [];
    const planItems: DropdownItem[] = [];
    for (const model of models) {
      if (planIds.has(model.id)) {
        planItems.push(toItem(model.id));
      } else {
        otherItems.push(toItem(model.id));
      }
    }
    const catalogEntries: DropdownEntry[] = [...otherItems];
    // See the comment on PLAN_TAG for OpenAI's in-use guideline.
    if (planItems.length > 0) {
      catalogEntries.push({ heading: PLAN_TAG, items: planItems });
    }
    const signIn: DropdownItem = {
      label: EMPTY_CATALOG_LABEL,
      onClick: () => {
        void this.account?.signIn();
      },
    };
    const items: DropdownEntry[] =
      models.length === 0
        ? [signIn]
        : this.account !== undefined && !this.account.signedIn
          ? [signIn, ...catalogEntries]
          : catalogEntries;
    this.dropdown.show(this.element, items, undefined, {
      skin: "composer",
      className: "ws-model-menu",
      placement: "above",
    });
  }

  private select(id: string): void {
    // The send result needs no local handling: the selection changes only
    // when the server's snapshot arrives, so a failed send simply leaves
    // the label unchanged.
    this.modelService.setCurrent(id);
  }

  private renderCurrent(): void {
    const current = this.modelService.current;
    this.labelSlot.textContent = current === "" ? NO_SELECTION_LABEL : current;
  }
}
