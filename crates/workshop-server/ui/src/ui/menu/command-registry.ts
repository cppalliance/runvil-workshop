// The command registry: a Map of command descriptors keyed by command
// id (the VS Code registerAction2 pattern). Every invocable action -
// menu rows, keyboard shortcuts, future command-palette entries - is a
// descriptor registered once and dispatched by id, so the surfaces that
// trigger a command never name the code that performs it. Feature
// directories register their commands from their register() functions;
// the composition root registers the built-in menu commands.
//
// The module-level Commands instance is the shared registry the
// composition root and feature directories populate; the MenuRenderer
// and the shortcut dispatcher read it. Tests construct their own
// instances for isolation.

import { toDisposable, type IDisposable } from "../../base/lifecycle";

/** One invocable action. */
export interface CommandDescriptor {
  /** The menu row label; optional for keyboard-only commands. */
  readonly label?: string;
  /** The shortcut hint rendered beside the label, e.g. "Ctrl+B". */
  readonly shortcut?: string;
  readonly run: () => void;
  /** Rows render disabled when this returns false; default enabled. */
  readonly enabled?: () => boolean;
}

export class CommandRegistry {
  private readonly commands = new Map<string, CommandDescriptor>();

  /**
   * Registers `descriptor` under `id`. Re-registering an id upserts: the
   * new descriptor replaces the old, so re-running a setup (a test
   * scenario, a hot reload) never duplicates. The returned disposable
   * unregisters only if this registration is still current, so disposing
   * a stale registration cannot evict its replacement.
   */
  register(id: string, descriptor: CommandDescriptor): IDisposable {
    this.commands.set(id, descriptor);
    return toDisposable(() => {
      if (this.commands.get(id) === descriptor) {
        this.commands.delete(id);
      }
    });
  }

  /** The descriptor registered under `id`, if any. */
  lookup(id: string): CommandDescriptor | undefined {
    return this.commands.get(id);
  }

  /**
   * Runs the command registered under `id`. Answers false when no command
   * is registered - a shortcut whose owning feature has not activated yet
   * is a no-op, never a crash.
   */
  execute(id: string): boolean {
    const descriptor = this.commands.get(id);
    if (descriptor === undefined) {
      return false;
    }
    descriptor.run();
    return true;
  }
}

/** The shared registry the running app dispatches through. */
export const Commands = new CommandRegistry();

/** Registers a command into the shared registry. */
export function registerCommand(id: string, descriptor: CommandDescriptor): IDisposable {
  return Commands.register(id, descriptor);
}

/** Dispatches a command by id through the shared registry. */
export function executeCommand(id: string): boolean {
  return Commands.execute(id);
}
