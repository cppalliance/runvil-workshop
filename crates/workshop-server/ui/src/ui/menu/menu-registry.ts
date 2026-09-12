// The menu registry: a Map of menu placements per menu id (the VS Code
// MenuRegistry pattern). A placement says which menu an item belongs to
// and where it sits; the item itself is either a reference to a command
// in the command registry or a separator. Menus whose rows are dynamic
// (the Model menu mirrors the live catalog) register a provider instead
// of static items, and the renderer re-reads it at every open and on
// every change while open.
//
// The module-level Menus instance is the shared registry the composition
// root and feature directories populate; tests construct their own.

import type { Event } from "../../base/event";
import { toDisposable, type IDisposable } from "../../base/lifecycle";

/** A menu row that dispatches a command from the command registry. */
export interface MenuCommandItemSpec {
  readonly kind: "command";
  readonly command: string;
}

/** A horizontal rule between rows. */
export interface MenuSeparatorSpec {
  readonly kind: "separator";
}

export type MenuItemSpec = MenuCommandItemSpec | MenuSeparatorSpec;

/**
 * One menu row, fully resolved for the renderer: the command descriptor
 * merged with the presentation extras the dynamic menus need (the Model
 * menu's radio checks, pending marks, tooltips, and the stable row key
 * the renderer uses to keep keyboard focus across a rebuild).
 */
export interface ResolvedCommandItem {
  readonly kind: "command";
  /** Stable identity for focus restoration across a rebuild. */
  readonly key?: string;
  readonly label: string;
  readonly shortcut?: string;
  readonly run: () => void;
  readonly enabled?: () => boolean;
  /** Radio rows: the check mark state. Absent for plain rows. */
  readonly checked?: boolean;
  /** A switch in flight: the pending mark and aria-busy. */
  readonly pending?: boolean;
  readonly tooltip?: string;
}

export type ResolvedMenuItem = ResolvedCommandItem | MenuSeparatorSpec;

/**
 * The rows of a dynamic menu, re-read at every open. onDidChange, when
 * present, fires while the menu is open to rebuild the rows in place.
 */
export interface MenuItemsProvider {
  readonly items: () => readonly ResolvedMenuItem[];
  readonly onDidChange?: Event<unknown>;
}

interface MenuDeclaration {
  readonly label: string;
  readonly order: number;
}

interface PlacedItem {
  readonly id: string;
  readonly spec: MenuItemSpec;
}

export class MenuRegistry {
  private readonly menus = new Map<string, MenuDeclaration>();
  private readonly items = new Map<string, PlacedItem[]>();
  private readonly providers = new Map<string, MenuItemsProvider>();

  /**
   * Declares one title-bar menu. `order` sorts the menus left to right,
   * matching the title bar's button order. Re-registering upserts.
   */
  registerMenu(id: string, label: string, order: number): IDisposable {
    const declaration: MenuDeclaration = { label, order };
    this.menus.set(id, declaration);
    return toDisposable(() => {
      if (this.menus.get(id) === declaration) {
        this.menus.delete(id);
        this.items.delete(id);
        this.providers.delete(id);
      }
    });
  }

  /**
   * Places an item in a menu (the appendMenuItem API). `itemId` is
   * stable: re-setting it upserts in place, keeping the item's position,
   * so re-running a setup never duplicates rows.
   */
  setMenuItem(menu: string, itemId: string, spec: MenuItemSpec): IDisposable {
    let list = this.items.get(menu);
    if (list === undefined) {
      list = [];
      this.items.set(menu, list);
    }
    const existing = list.findIndex((item) => item.id === itemId);
    const placed: PlacedItem = { id: itemId, spec };
    if (existing === -1) {
      list.push(placed);
    } else {
      list[existing] = placed;
    }
    return toDisposable(() => {
      const current = this.items.get(menu);
      const index = current?.findIndex((item) => item.id === itemId && item.spec === spec) ?? -1;
      if (current !== undefined && index !== -1) {
        current.splice(index, 1);
      }
    });
  }

  /** Registers the dynamic row source for a menu; replaces any previous. */
  setProvider(menu: string, provider: MenuItemsProvider): IDisposable {
    this.providers.set(menu, provider);
    return toDisposable(() => {
      if (this.providers.get(menu) === provider) {
        this.providers.delete(menu);
      }
    });
  }

  /** The declared menus, sorted into title-bar order. */
  menusInOrder(): readonly { readonly id: string; readonly label: string }[] {
    return [...this.menus.entries()]
      .sort(([, a], [, b]) => a.order - b.order)
      .map(([id, declaration]) => ({ id, label: declaration.label }));
  }

  /** A menu's placed items, in position order. */
  itemSpecs(menu: string): readonly PlacedItem[] {
    return this.items.get(menu) ?? [];
  }

  /** A menu's dynamic row source, if it has one. */
  providerFor(menu: string): MenuItemsProvider | undefined {
    return this.providers.get(menu);
  }
}

/** The shared registry the running app renders menus from. */
export const Menus = new MenuRegistry();

/** Places an item in a menu of the shared registry. */
export function appendMenuItem(menu: string, itemId: string, spec: MenuItemSpec): IDisposable {
  return Menus.setMenuItem(menu, itemId, spec);
}
