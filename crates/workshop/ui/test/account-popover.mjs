// A signed-in account item opens its popup above the status bar, inside
// the viewport. Run after `node build.mjs`.
import { bootWorkbench } from "./helpers/boot.mjs";

const accountView = {
  state: "signed_in",
  email: "a@example.com",
  planGranted: true,
  detail: "Signed in",
  planModels: ["gpt-6.1-sol"],
  accountStatus: "active",
};

await bootWorkbench(
  "the account popup opens above the status bar",
  async (ctx) => {
    const { document, window, indicatorsEl, failures, shadowRoots, sleep } = ctx;
    const item = document.querySelector('[data-indicator="chatgpt-account"]');
    if (item === null || item.hidden) {
      failures.push("the signed-in account item is missing");
      return;
    }
    const order = [...(indicatorsEl?.children ?? [])].map((el) => el.dataset.indicator).join(",");
    if (order !== "chatgpt-account,mic-meter,recording,activity") {
      failures.push(`the account item is not left of the meter and LEDs: ${order}`);
    }
    window.innerWidth = 800;
    window.innerHeight = 600;
    item.getBoundingClientRect = () => ({
      x: 520,
      y: 578,
      width: 140,
      height: 22,
      top: 578,
      left: 520,
      right: 660,
      bottom: 600,
      toJSON() {
        return this;
      },
    });
    item.click();
    await sleep(0);
    const statusEl = item.querySelector(".ws-chatgpt-status");
    const root = shadowRoots.get(statusEl);
    const pop = root?.querySelector(".ws-chatgpt-status__pop");
    if (pop === null || pop === undefined) {
      failures.push("the account popup did not open");
      return;
    }
    const bar = root.querySelector(".ws-chatgpt-status__bar");
    if (bar?.textContent !== "ChatGPT plan") {
      failures.push(`the bar is not the two words: ${bar?.textContent ?? "missing"}`);
    }
    if (bar?.title !== "a@example.com") {
      failures.push(`the email is not the tooltip: ${bar?.title ?? "missing"}`);
    }
    if (bar?.querySelector(".email") !== null) {
      failures.push("the email is still painted in the bar");
    }
    if (item.textContent?.includes("a@example.com") || item.innerHTML.includes("a@example.com")) {
      failures.push("the email leaked out of the closed shadow");
    }
    if (item.getAttribute("aria-label") !== "ChatGPT plan" || item.title !== "") {
      failures.push(`the status button names more than the two words: ${item.getAttribute("aria-label")} / ${item.title}`);
    }
    if (pop.textContent?.trim() !== "Sign out") {
      failures.push(`the popup is not only Sign out: ${pop.textContent}`);
    }
    if (pop.textContent?.includes("a@example.com") || pop.textContent?.includes("Plan active")) {
      failures.push("the popup still shows the email or a state line");
    }
    const fallbackWidth = 180;
    const top = Number.parseFloat(pop.style.top);
    const left = Number.parseFloat(pop.style.left);
    if (!(top >= 8 && top < 578)) {
      failures.push(`the popup does not sit above the bar inside the viewport: top=${top}`);
    }
    if (!(left >= 8 && left + fallbackWidth <= window.innerWidth - 8)) {
      failures.push(`the popup is outside the viewport horizontally: left=${left}`);
    }

    // Close and reopen near the left edge of a narrow viewport so left-edge clamping is proven too.
    item.click();
    await sleep(0);
    window.innerWidth = 320;
    item.getBoundingClientRect = () => ({
      x: 16,
      y: 578,
      width: 104,
      height: 22,
      top: 578,
      left: 16,
      right: 120,
      bottom: 600,
      toJSON() {
        return this;
      },
    });
    item.click();
    await sleep(0);
    const narrowPop = root?.querySelector(".ws-chatgpt-status__pop");
    const narrowLeft = Number.parseFloat(narrowPop?.style.left ?? "NaN");
    if (!(narrowLeft >= 8 && narrowLeft + fallbackWidth <= window.innerWidth - 8)) {
      failures.push(`the popup overflows a narrow viewport when the item is further left: left=${narrowLeft}`);
    }

    document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await sleep(0);
    if (root?.querySelector(".ws-chatgpt-status__pop") !== null) {
      failures.push("Escape did not close the account popup");
    }

    item.click();
    await sleep(0);
    document.body.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
    await sleep(0);
    if (root?.querySelector(".ws-chatgpt-status__pop") !== null) {
      failures.push("an outside pointerdown did not close the account popup");
    }
  },
  { frozenInvoke: true, captureShadows: true, accountView },
);
