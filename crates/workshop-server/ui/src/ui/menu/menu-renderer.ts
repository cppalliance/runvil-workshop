// The menu renderer: reads the command and menu registries and builds
// the title-bar popovers - accessible HTML menus behind the File, Edit,
// Model, Window, and Help buttons. The renderer knows nothing about what
// is registered: menus, items, and dynamic providers all come from the
// two registries, so a feature directory adding a command and a placement
// changes the menus without touching this file. Interaction behavior
// (one-menu-at-a-time, menubar rollover, arrow/Enter/Escape navigation,
// close on outside pointer and window blur, focus restoration across a
// dynamic rebuild) is identical to the pre-split window-menu.ts.

import "./window-menu.css";

import { Disposable, toDisposable, type IDisposable } from "../../base/lifecycle";
import { Commands, type CommandRegistry } from "./command-registry";
import {
  Menus,
  type MenuRegistry,
  type ResolvedCommandItem,
  type ResolvedMenuItem,
} from "./menu-registry";

interface CommandRow {
  readonly element: HTMLButtonElement;
  readonly labelElement: HTMLSpanElement;
  readonly def: ResolvedCommandItem;
}

interface MenuHandle {
  readonly id: string;
  readonly button: HTMLButtonElement;
  readonly popover: HTMLElement;
  // Mutable: provider menus rebuild their rows on every open, so the
  // array cannot be frozen at build time.
  rows: CommandRow[];
}

/**
 * Builds and drives the title-bar menus from the registries. Throws if
 * the title-bar markup is missing a registered menu's button. dispose()
 * releases the document-level listeners and removes the popovers (whose
 * removal also drops every row's element-owned click listener).
 */
export class MenuRenderer extends Disposable {
  private readonly handles: MenuHandle[] = [];
  private openId: string | null = null;
  // Alive only while a provider menu is open: each change rebuilds the
  // rows in place, so check and pending marks move without reopening.
  private providerWatch: IDisposable | null = null;

  constructor(
    nav: HTMLElement,
    private readonly commands: CommandRegistry = Commands,
    private readonly menus: MenuRegistry = Menus,
  ) {
    super();
    this._register(
      toDisposable(() => {
        this.providerWatch?.dispose();
        this.providerWatch = null;
      }),
    );

    for (const menu of menus.menusInOrder()) {
      this.handles.push(this.buildPopover(nav, menu.id, menu.label));
    }

    for (const handle of this.handles) {
      const onButtonClick = (): void => {
        if (this.openId === handle.id) {
          this.closeMenu();
        } else {
          this.openMenu(handle, false);
        }
      };
      handle.button.addEventListener("click", onButtonClick);
      this._register(toDisposable(() => handle.button.removeEventListener("click", onButtonClick)));
      // Menubar rollover: while any menu is open, hovering another button
      // switches the open menu to it. With no menu open, hover alone
      // opens nothing.
      const onButtonEnter = (): void => {
        if (this.openId !== null && this.openId !== handle.id) {
          this.openMenu(handle, false);
        }
      };
      handle.button.addEventListener("pointerenter", onButtonEnter);
      this._register(
        toDisposable(() => handle.button.removeEventListener("pointerenter", onButtonEnter)),
      );
    }

    const onPointerDown = (event: PointerEvent): void => {
      if (this.openId === null) {
        return;
      }
      const handle = this.handleFor(this.openId);
      const target = event.target;
      if (!(target instanceof Node)) {
        return;
      }
      if (handle.popover.contains(target) || handle.button.contains(target)) {
        return;
      }
      this.closeMenu();
    };
    document.addEventListener("pointerdown", onPointerDown);
    this._register(toDisposable(() => document.removeEventListener("pointerdown", onPointerDown)));

    // Close the menu when the window loses focus (Alt+Tab, taskbar click,
    // notification popup) so a stale popover never covers a returned
    // window.
    const onWindowBlur = (): void => this.closeMenu();
    window.addEventListener("blur", onWindowBlur);
    this._register(toDisposable(() => window.removeEventListener("blur", onWindowBlur)));

    const onKeydown = (event: KeyboardEvent): void => {
      if (this.openId === null) {
        return;
      }
      const handle = this.handleFor(this.openId);
      switch (event.key) {
        case "Escape":
          event.preventDefault();
          this.closeMenu();
          handle.button.focus();
          break;
        case "ArrowDown":
          event.preventDefault();
          this.moveFocus(handle, 1);
          break;
        case "ArrowUp":
          event.preventDefault();
          this.moveFocus(handle, -1);
          break;
        case "ArrowRight":
          event.preventDefault();
          this.stepMenu(1);
          break;
        case "ArrowLeft":
          event.preventDefault();
          this.stepMenu(-1);
          break;
        case "Enter": {
          const row = handle.rows.find(
            (candidate) => candidate.element === document.activeElement,
          );
          if (row) {
            event.preventDefault();
            this.activateRow(row);
          }
          break;
        }
      }
    };
    document.addEventListener("keydown", onKeydown);
    this._register(toDisposable(() => document.removeEventListener("keydown", onKeydown)));
  }

  private handleFor(id: string): MenuHandle {
    const handle = this.handles.find((candidate) => candidate.id === id);
    if (!handle) {
      throw new Error(`DOM Error: the ${id} menu was not built.`);
    }
    return handle;
  }

  /** One menu's rows at build or open time, resolved from the registries. */
  private resolveItems(id: string): readonly ResolvedMenuItem[] {
    const provider = this.menus.providerFor(id);
    if (provider !== undefined) {
      return provider.items();
    }
    return this.menus.itemSpecs(id).map((placed) => {
      if (placed.spec.kind === "separator") {
        return { kind: "separator" };
      }
      const descriptor = this.commands.lookup(placed.spec.command);
      if (descriptor === undefined) {
        // A placement without its command is a registration-order bug;
        // render it disabled and labelled rather than dropping the row
        // silently or crashing the menubar.
        return {
          kind: "command",
          key: placed.id,
          label: placed.spec.command,
          run: () => undefined,
          enabled: () => false,
        };
      }
      return {
        kind: "command",
        key: placed.id,
        label: descriptor.label ?? placed.spec.command,
        shortcut: descriptor.shortcut,
        run: descriptor.run,
        enabled: descriptor.enabled,
      };
    });
  }

  private refreshEnabled(handle: MenuHandle): void {
    for (const row of handle.rows) {
      const enabled = row.def.enabled ? row.def.enabled() : true;
      row.element.setAttribute("aria-disabled", enabled ? "false" : "true");
      row.labelElement.textContent = row.def.label;
    }
  }

  private activateRow(row: CommandRow): void {
    if (row.element.getAttribute("aria-disabled") === "true") {
      return;
    }
    // Close first and return focus to the menu button, so a command that
    // opens a surface (the About dialog) records a visible invoker to
    // restore focus to on dismissal.
    const handle = this.openId === null ? null : this.handleFor(this.openId);
    this.closeMenu();
    handle?.button.focus();
    row.def.run();
  }

  // One command row: a menuitem button with its label, an optional
  // shortcut hint, and click dispatch through activateRow. Checkable rows
  // (the Model menu's radio items) carry the check column before the
  // label in layout order.
  private buildCommandRow(def: ResolvedCommandItem): CommandRow {
    const element = document.createElement("button");
    element.type = "button";
    element.className = "ws-window-titlebar__item";
    element.setAttribute("role", "menuitem");
    element.setAttribute("aria-disabled", "true");
    if (def.key !== undefined) {
      element.dataset["menuRowKey"] = def.key;
    }
    if (def.checked !== undefined) {
      element.classList.add("ws-window-titlebar__item--checkable");
      element.setAttribute("role", "menuitemradio");
      element.setAttribute("aria-checked", def.checked ? "true" : "false");
      if (def.tooltip) {
        element.title = def.tooltip;
      }
      const check = document.createElement("span");
      check.className = "ws-window-titlebar__item-check";
      if (def.pending) {
        check.classList.add("ws-window-titlebar__item-check--pending");
        // The "…" mark is aria-hidden and aria-checked stays false until
        // the server confirms, so without this the switch target is
        // indistinguishable from the other disabled rows for assistive
        // tech. The next settle rebuilds rows without it.
        element.setAttribute("aria-busy", "true");
      }
      check.setAttribute("aria-hidden", "true");
      check.textContent = def.pending ? "…" : def.checked ? "✓" : "";
      element.appendChild(check);
    }
    const label = document.createElement("span");
    label.className = "ws-window-titlebar__item-label";
    label.textContent = def.label;
    element.appendChild(label);
    if (def.shortcut) {
      const shortcut = document.createElement("span");
      shortcut.className = "ws-window-titlebar__shortcut";
      shortcut.textContent = def.shortcut;
      element.appendChild(shortcut);
    }
    const row: CommandRow = { element, labelElement: label, def };
    element.addEventListener("click", () => this.activateRow(row));
    return row;
  }

  private buildPopover(nav: HTMLElement, id: string, label: string): MenuHandle {
    const button = nav.querySelector<HTMLButtonElement>(`[data-menu="${id}"]`);
    if (!button) {
      throw new Error(`DOM Error: the title bar is missing the ${label} menu button.`);
    }
    const popover = document.createElement("div");
    popover.className = "ws-window-titlebar__popover";
    popover.setAttribute("role", "menu");
    popover.setAttribute("aria-label", label);
    popover.hidden = true;
    const handle: MenuHandle = { id, button, popover, rows: [] };
    // Provider menus build their rows at open time; static menus build
    // once here and only refresh enabled state at open.
    if (this.menus.providerFor(id) === undefined) {
      this.rebuildRows(handle);
    }
    button.insertAdjacentElement("afterend", popover);
    this._register(toDisposable(() => popover.remove()));
    return handle;
  }

  /**
   * Rebuilds a menu's rows from its current items. Wiping the popover
   * destroys the focused row and drops keyboard focus to body, so a
   * change arriving mid-navigation would yank a screen-reader user's
   * position: the focused row is remembered by its stable identity (the
   * row key, never the index) and focus lands on the equivalent new row.
   */
  private rebuildRows(handle: MenuHandle): void {
    const focusedKey = handle.rows.find(
      (row) => row.element === document.activeElement,
    )?.element.dataset["menuRowKey"];
    handle.popover.textContent = "";
    handle.rows.length = 0;
    for (const def of this.resolveItems(handle.id)) {
      if (def.kind === "separator") {
        const separator = document.createElement("div");
        separator.className = "ws-window-titlebar__separator";
        separator.setAttribute("role", "separator");
        handle.popover.appendChild(separator);
        continue;
      }
      const row = this.buildCommandRow(def);
      handle.rows.push(row);
      handle.popover.appendChild(row.element);
    }
    // Only when a row held focus before the wipe: a rebuild while focus
    // is elsewhere must not grab it. The items may have dropped the
    // focused row entirely; the first row is the fallback.
    if (focusedKey !== undefined) {
      const restored =
        handle.rows.find((row) => row.element.dataset["menuRowKey"] === focusedKey) ??
        handle.rows[0];
      restored?.element.focus();
    }
  }

  private openMenu(handle: MenuHandle, focusFirst: boolean): void {
    this.closeMenu();
    const provider = this.menus.providerFor(handle.id);
    if (provider !== undefined) {
      this.rebuildRows(handle);
      this.providerWatch =
        provider.onDidChange?.(() => {
          this.rebuildRows(handle);
          this.refreshEnabled(handle);
        }) ?? null;
    }
    this.refreshEnabled(handle);
    // Align the popover under its button; absolute positioning keeps the
    // bar's layout unchanged.
    handle.popover.style.left = `${handle.button.offsetLeft}px`;
    handle.popover.style.top = `${handle.button.offsetTop + handle.button.offsetHeight}px`;
    handle.popover.hidden = false;
    handle.button.setAttribute("aria-expanded", "true");
    this.openId = handle.id;
    if (focusFirst) {
      this.focusRow(handle, 0);
    }
  }

  private closeMenu(): void {
    if (this.openId === null) {
      return;
    }
    this.providerWatch?.dispose();
    this.providerWatch = null;
    const handle = this.handleFor(this.openId);
    handle.popover.hidden = true;
    handle.button.setAttribute("aria-expanded", "false");
    this.openId = null;
  }

  private focusRow(handle: MenuHandle, index: number): void {
    const row = handle.rows[index];
    if (row) {
      row.element.focus();
    }
  }

  private moveFocus(handle: MenuHandle, delta: number): void {
    const count = handle.rows.length;
    if (count === 0) {
      return;
    }
    const current = handle.rows.findIndex((row) => row.element === document.activeElement);
    const next =
      current === -1 ? (delta > 0 ? 0 : count - 1) : (current + delta + count) % count;
    this.focusRow(handle, next);
  }

  private stepMenu(delta: number): void {
    if (this.openId === null) {
      return;
    }
    const order = this.menus.menusInOrder();
    const current = order.findIndex((menu) => menu.id === this.openId);
    const next = order[(current + delta + order.length) % order.length];
    if (next) {
      this.openMenu(this.handleFor(next.id), true);
    }
  }
}
