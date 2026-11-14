// The ChatGPT account, one value for the whole app.
//
// The desktop process owns the proof and the browser. This service asks
// it over IPC and keeps the view every window reads. The view never
// carries a proof, a token, or an authorize URL.

import { Emitter, type Event } from "@workshop/platform/event";
import { Disposable, toDisposable } from "@workshop/platform/lifecycle";
import { createServiceToken, type ServiceToken } from "@workshop/platform/service-registry";
import type { CatalogModel } from "./protocol";

/**
 * One of the four button formats OpenAI lists ("Continue with ChatGPT" or "Sign in with ChatGPT", each with a white or a black logo), section "1. Add the sign-in button":
 * https://developers.openai.com/siwc/website
 * Workshop uses the label as text only.
 */
export const SIGN_IN_LABEL = "Sign in with ChatGPT";

export type AccountState =
  | "signed_out"
  | "waiting"
  | "signed_in"
  | "needs_sign_in"
  | "unavailable"
  | "unreadable";

export interface AccountView {
  readonly state: AccountState;
  readonly email: string | null;
  readonly planGranted: boolean;
  readonly detail: string;
  readonly planModels: readonly string[];
  readonly accountStatus?: string | null;
  readonly revocation?: string | null;
  readonly cleanupUnconfirmed?: boolean;
  readonly unavailableReason?: string | null;
}

/** The mark is left out until OpenAI's permission is settled; see the comment on `SIGN_IN_LABEL`. */
export const OPENAI_CHATGPT_MARK_SVG: string | null = null;

/**
 * OpenAI's UI/UX guidelines for apps that use a person's ChatGPT plan, section "Show when the ChatGPT plan is in use": "When requests use the user's ChatGPT plan, display Using ChatGPT plan near the composer or model selector."
 * https://developers.openai.com/siwc/ui-ux-guidelines
 * Workshop shows the shorter "ChatGPT plan", as the heading over plan models in the model picker and as the status bar item. The same section asks for a "Manage usage" link to ChatGPT's usage settings; Workshop does not have one yet.
 */
export const PLAN_TAG = "ChatGPT plan";

export type AccountButton =
  | "sign-out"
  | "start-over"
  | "sign-in-again"
  | "reset"
  | "try-again"
  | "check-again"
  | "dismiss";

export interface AccountChip {
  readonly hidden: boolean;
  /** Always the two words `ChatGPT plan` while the item is visible. */
  readonly label: string;
  /** The email when the plan is ready; otherwise the short state line. */
  readonly tooltip: string;
  readonly warn: boolean;
  readonly button: AccountButton | null;
  readonly buttonDisabled: boolean;
}

export interface AccountChipFlags {
  readonly enabling: boolean;
  readonly enableProblem: "conflict" | "failed" | null;
  readonly dismissedCleanup: boolean;
}

const HIDDEN: AccountChip = {
  hidden: true,
  label: "",
  tooltip: "",
  warn: false,
  button: null,
  buttonDisabled: false,
};

/** Every visible state uses the same two words. The popover is only `button`. */
function visible(tooltip: string, button: AccountButton, warn = false, buttonDisabled = false): AccountChip {
  return { hidden: false, label: PLAN_TAG, tooltip, warn, button, buttonDisabled };
}

function unavailableLine(reason: string | null | undefined): string {
  const text = (reason ?? "").toLowerCase();
  if (text.includes("loopback")) {
    return "Requires loopback Gateway address.";
  }
  if (text.includes("profile") || text.includes("folder") || text.includes("directory")) {
    return "No writable user folder found.";
  }
  if (text.includes("storage") || text.includes("windows") || text.includes("unsupported")) {
    return "Sign-in storage unavailable.";
  }
  return "Local Gateway unavailable.";
}

const CLEANUP_LINE = "Remote sign-out unconfirmed.";

function cleanupPending(view: AccountView, flags: AccountChipFlags): boolean {
  return (view.revocation === "unconfirmed" || view.cleanupUnconfirmed === true) && !flags.dismissedCleanup;
}

/**
 * The status item. Signed out with nothing pending is hidden.
 * The bar is always `ChatGPT plan`. A ready plan puts the email on the
 * tooltip; every other visible state puts the short state line there.
 * An unconfirmed remote cleanup stays visible on an active account too,
 * until this session dismisses it.
 */
export function accountChip(view: AccountView, flags: AccountChipFlags): AccountChip {
  const chip = chipFor(view, flags);
  if (!cleanupPending(view, flags)) {
    return chip;
  }
  if (chip.hidden) {
    return visible(CLEANUP_LINE, "dismiss", true);
  }
  return { ...chip, warn: true, tooltip: CLEANUP_LINE };
}

function chipFor(view: AccountView, flags: AccountChipFlags): AccountChip {
  if (view.state === "unavailable") {
    return visible(unavailableLine(view.unavailableReason ?? view.detail), "check-again", true);
  }
  if (view.state === "unreadable") {
    return visible("Saved sign-in is unreadable.", "reset", true);
  }
  if (view.state === "waiting") {
    return visible("Finish sign-in in browser.", "start-over");
  }
  if (view.state === "signed_in") {
    if (flags.enabling) {
      return visible("Adding plan models...", "sign-out", false, true);
    }
    if (flags.enableProblem === "conflict") {
      return visible("Apply settings changes first.", "try-again");
    }
    if (flags.enableProblem === "failed") {
      return visible("Could not add plan models.", "try-again");
    }
    return visible(view.email ?? "", "sign-out");
  }
  if (view.state === "needs_sign_in" && view.accountStatus === "active") {
    return visible("Account lacks plan access.", "sign-in-again", true);
  }
  if (view.state === "needs_sign_in") {
    return visible("Sign-in expired.", "sign-in-again", true);
  }
  return HIDDEN;
}

const BUTTON_LABEL: Record<AccountButton, string> = {
  "sign-out": "Sign out",
  "start-over": "Start over",
  "sign-in-again": "Sign in again",
  reset: "Reset",
  "try-again": "Try again",
  "check-again": "Check again",
  dismiss: "Dismiss",
};

export function accountButtonLabel(button: AccountButton): string {
  return BUTTON_LABEL[button];
}

export interface RenamedModel {
  readonly from: string;
  readonly to: string;
}

export interface SkippedModel {
  readonly name: string;
  readonly reason: string;
}

export interface EnableModelsReply {
  readonly endpoint: string;
  readonly added: readonly string[];
  readonly renamed: readonly RenamedModel[];
  readonly skipped: readonly SkippedModel[];
}

export interface AccountBackend {
  status(): Promise<AccountView>;
  signIn(): Promise<AccountView>;
  signOut(): Promise<AccountView>;
  reset(): Promise<AccountView>;
  enable(): Promise<EnableModelsReply>;
}

const EMPTY: AccountView = {
  state: "signed_out",
  email: null,
  planGranted: false,
  detail: "Signed out",
  planModels: [],
};

interface InvokeTarget {
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
}

/**
 * The account caller. It closes over `target.invoke` and a capability.
 *
 * Tauri 2.11 defines `__TAURI_INTERNALS__.invoke` with `Object.defineProperty`
 * and only a `value`, so the property is neither writable nor configurable.
 * Replacing it throws, and Tauri plugins call that same property after
 * boot. The public invoke therefore cannot refuse command names. A call
 * without this capability is refused by the desktop process.
 */
export function sealAccountInvoke(target: InvokeTarget): (command: string, args?: Record<string, unknown>) => Promise<unknown> {
  const raw = target.invoke.bind(target);
  const capability = newCapability();
  return (command, args) => raw(command, { ...args, capability });
}

function newCapability(): string {
  const bytes = new Uint8Array(16);
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    crypto.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** A round failure whose text names a ChatGPT sign-in code. */
export function isSignInRoundFailure(message: string): boolean {
  return message.includes("chatgpt_");
}

/**
 * The first plan id that is in `models`, when nothing is selected.
 * An existing selection is left alone, including an API-key model
 * whose plan copy was renamed to `<name>-chatgpt`.
 */
export function selectionAfterRefresh(
  current: string,
  models: readonly CatalogModel[],
  planIds: readonly string[],
): string | null {
  if (current !== "") {
    return null;
  }
  const present = new Set(models.map((model) => model.id));
  return planIds.find((id) => present.has(id)) ?? null;
}

/** How often a sign-in that is still in the browser re-reads status. */
const POLL_MS = 1000;

export class ChatgptAccount extends Disposable {
  private view: AccountView = EMPTY;
  private enabling = false;
  private enableProblem: "conflict" | "failed" | null = null;
  private dismissedCleanup = false;
  private autoEnabled = false;
  private enableGeneration = 0;
  private disposed = false;
  private flow = 0;
  private poll: ReturnType<typeof setTimeout> | null = null;
  private readonly _onDidChange = this._register(new Emitter<AccountView>());
  readonly onDidChange: Event<AccountView> = this._onDidChange.event;

  constructor(
    private readonly backend: AccountBackend,
    private readonly refreshModels: () => boolean,
    private readonly selectModel: (id: string) => boolean,
    private readonly models: () => readonly CatalogModel[],
    private readonly current: () => string,
    private readonly onModels?: Event<CatalogModel[]>,
    private readonly pollMs: number = POLL_MS,
  ) {
    super();
    this._register(
      toDisposable(() => {
        this.disposed = true;
        this.flow += 1;
        this.stopPoll();
      }),
    );
  }

  get snapshot(): AccountView {
    return this.view;
  }

  get signedIn(): boolean {
    return this.view.state === "signed_in";
  }

  get chip(): AccountChip {
    return accountChip(this.view, {
      enabling: this.enabling,
      enableProblem: this.enableProblem,
      dismissedCleanup: this.dismissedCleanup,
    });
  }

  dismissCleanup(): void {
    this.dismissedCleanup = true;
    this._onDidChange.fire(this.view);
  }

  async refresh(): Promise<void> {
    const flow = this.bump();
    const view = await this.capture(() => this.backend.status());
    if (view === null || this.disposed || this.flow !== flow) {
      return;
    }
    this.set(view);
    await this.follow();
  }

  async signIn(): Promise<void> {
    const flow = this.bump();
    this.autoEnabled = false;
    this.enableProblem = null;
    const view = await this.capture(() => this.backend.signIn());
    if (view === null || this.disposed || this.flow !== flow) {
      return;
    }
    this.set(view);
    await this.follow();
  }

  async signOut(): Promise<void> {
    const flow = this.bump();
    this.autoEnabled = false;
    const view = await this.capture(() => this.backend.signOut());
    if (view === null || this.disposed || this.flow !== flow) {
      return;
    }
    this.set(view);
    await this.follow();
  }

  async reset(): Promise<void> {
    const flow = this.bump();
    this.autoEnabled = false;
    this.enableProblem = null;
    const view = await this.capture(() => this.backend.reset());
    if (view === null || this.disposed || this.flow !== flow) {
      return;
    }
    this.set(view);
    await this.follow();
  }

  async enable(): Promise<void> {
    const flow = this.flow;
    const generation = ++this.enableGeneration;
    this.autoEnabled = true;
    this.enabling = true;
    this._onDidChange.fire(this.view);
    let problem: "conflict" | "failed" | null = null;
    try {
      await this.backend.enable();
    } catch (error) {
      const text = error instanceof Error ? error.message : String(error);
      problem = text.includes("config_has_pending_changes") ? "conflict" : "failed";
      console.error("ChatGPT account is off:", text);
    }
    if (this.disposed || this.flow !== flow || this.enableGeneration !== generation) {
      return;
    }
    this.enabling = false;
    this.enableProblem = problem;
    this._onDidChange.fire(this.view);
    await this.refresh();
  }

  async noteRoundFailure(message: string): Promise<void> {
    if (!isSignInRoundFailure(message)) {
      return;
    }
    await this.refresh();
  }

  /** Reads a view and does not publish it. `null` means the read failed. */
  private async capture(read: () => Promise<AccountView>): Promise<AccountView | null> {
    try {
      return await read();
    } catch (error) {
      console.error(
        "ChatGPT account is off:",
        error instanceof Error ? error.message : "unavailable",
      );
      return null;
    }
  }

  /** Waiting keeps polling. A granted plan with no models enables once. Anything else stops. */
  private async follow(): Promise<void> {
    if (this.disposed) {
      return;
    }
    if (this.view.state === "waiting") {
      this.armPoll();
      return;
    }
    this.stopPoll();
    if (this.shouldAutoEnable()) {
      await this.enable();
      return;
    }
    this.refreshAndSelect();
  }

  private shouldAutoEnable(): boolean {
    return (
      this.view.state === "signed_in" &&
      this.view.planGranted &&
      this.view.planModels.length === 0 &&
      !this.autoEnabled &&
      !this.enabling
    );
  }

  private bump(): number {
    this.flow += 1;
    this.stopPoll();
    this.retireEnable();
    return this.flow;
  }

  /** A new flow owns the busy flag. An older enable must not clear it. */
  private retireEnable(): void {
    this.enableGeneration += 1;
    if (!this.enabling) {
      return;
    }
    this.enabling = false;
    this._onDidChange.fire(this.view);
  }

  private armPoll(): void {
    if (this.poll !== null || this.disposed) {
      return;
    }
    const flow = this.flow;
    this.poll = setTimeout(() => {
      this.poll = null;
      void this.pollStatus(flow);
    }, this.pollMs);
  }

  private stopPoll(): void {
    if (this.poll !== null) {
      clearTimeout(this.poll);
      this.poll = null;
    }
  }

  private async pollStatus(flow: number): Promise<void> {
    if (this.disposed || flow !== this.flow || this.view.state !== "waiting") {
      return;
    }
    let view: AccountView;
    try {
      view = await this.backend.status();
    } catch (error) {
      console.error(
        "ChatGPT account is off:",
        error instanceof Error ? error.message : "unavailable",
      );
      if (!this.disposed && flow === this.flow && this.view.state === "waiting") {
        this.armPoll();
      }
      return;
    }
    if (this.disposed || flow !== this.flow) {
      return;
    }
    this.set(view);
    await this.follow();
  }

  /**
   * Asks every window for a fresh catalog, then selects the first plan
   * model only from a catalog that arrived after that ask.
   */
  private refreshAndSelect(): void {
    const planIds = this.view.planModels;
    const flow = this.flow;
    if (planIds.length === 0 || this.onModels === undefined) {
      if (this.disposed || this.flow !== flow) {
        return;
      }
      this.refreshModels();
      if (this.onModels === undefined) {
        this.selectPlanIfEmpty();
      }
      return;
    }
    const sub = this.onModels((models) => {
      if (this.disposed || this.flow !== flow) {
        sub.dispose();
        return;
      }
      const id = selectionAfterRefresh(this.current(), models, planIds);
      if (id === null && this.current() === "") {
        return;
      }
      sub.dispose();
      if (id !== null) {
        this.selectModel(id);
      }
    });
    this._register(sub);
    if (this.disposed || this.flow !== flow || !this.refreshModels()) {
      sub.dispose();
    }
  }

  private selectPlanIfEmpty(): void {
    const id = selectionAfterRefresh(this.current(), this.models(), this.view.planModels);
    if (id !== null) {
      this.selectModel(id);
    }
  }

  private set(view: AccountView): void {
    if (view.revocation !== "unconfirmed" && view.cleanupUnconfirmed !== true) {
      this.dismissedCleanup = false;
    }
    this.view = view;
    this._onDidChange.fire(view);
  }
}

export const CHATGPT_ACCOUNT: ServiceToken<ChatgptAccount> =
  createServiceToken<ChatgptAccount>("workshop.chatgptAccount");
