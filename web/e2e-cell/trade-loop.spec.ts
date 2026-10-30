import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { type Locator, type Page, expect, test } from "@playwright/test";
import { deferredCheckLabel } from "../src/lib/deferred-checks";

/**
 * The trade loop from web/, end to end in the local cell (the founder's 2026-10-27 deadline), on the
 * chat screens (DEC-2293): a deployed paper agent asks for the owner's OK in its thread, the owner
 * approves it in the browser with the code the host confirms (`cli_confirm`, DEC-2104), the executor
 * places and fills the order on the fixture broker, the agent says so in its thread, its Details
 * show the position and the filled order from the served reads (DEC-2377), the owner's exit from
 * Details is recorded and its sell is said in the thread too, the browser's kill switch is recorded,
 * and the agent then reads as stopped.
 *
 * The driver sets it up and plays the host. It runs this file with `OWLHEAD_CELL_SPEC=trade-loop.spec.ts`
 * and the agent in `OWLHEAD_CELL_AGENT`. The spec triggers nothing itself: it waits for a pending
 * request from that agent, and only that agent, which the runtime raises on its own. When the page shows a code, the spec writes
 * `{workspace, principal, agent, approval, challenge, code}` to `OWLHEAD_CELL_CONFIRM_REQUEST`. The
 * driver runs `mandate step-up confirm` with those flags and then writes `OWLHEAD_CELL_CONFIRM_DONE`,
 * either `{"ok": true}` or `{"ok": false, "error": …}`. The spec writes each step it reached, and the
 * ids it recorded, to `OWLHEAD_CELL_RESULT`, so a failure names the first step that did not happen.
 * Paper only: no step here can reach a live venue.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const WORKSPACE = process.env.OWLHEAD_WORKSPACE_ID ?? "";
const AGENT = process.env.OWLHEAD_CELL_AGENT ?? "";
const CONFIRM_REQUEST = process.env.OWLHEAD_CELL_CONFIRM_REQUEST ?? "";
const CONFIRM_DONE = process.env.OWLHEAD_CELL_CONFIRM_DONE ?? "";
/** How long each wait on the deployment may take: the runtime's proposal, the host, the fill. */
const PROPOSAL_MS = Number(process.env.OWLHEAD_CELL_PROPOSAL_MS ?? 120_000);
const CONFIRM_MS = Number(process.env.OWLHEAD_CELL_CONFIRM_MS ?? 60_000);
const FILL_MS = Number(process.env.OWLHEAD_CELL_FILL_MS ?? 120_000);

/**
 * The sentence the approver is shown above the checks the dry run deferred, read from the journal
 * vectors so the spec cannot drift from it (DEC-2515 item 5). The driver runs this from `web/`.
 */
const APPROVER_SENTENCE = (JSON.parse(readFileSync(join(process.cwd(), "../fixtures/refcases/journal.json"), "utf8")) as { approval_deferrals: { approver_sentence: string } }).approval_deferrals.approver_sentence;

/** The Details drawer's fixed sentences for a section with nothing listed (`details.tsx`), the only text a failure may quote from it. */
const SECTION_STATES = ["Reading…", "Holds nothing.", "No orders yet.", "Your deployment didn't report this. It is read again when you reopen Details."];

/** The agent's thread, and the cards its runtime posts there (`chat_watch.rs`). */
const cards = (page: Page, kind: string) => page.locator(`[data-slot=chat-card][data-kind=${kind}]`);

/** Opens the agent's Details drawer, closing one already open first so its reads run again. */
async function openDetails(page: Page): Promise<Locator> {
  const drawer = page.locator("[data-slot=details-drawer]");
  if (await drawer.isVisible()) await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Details", exact: true }).click();
  await expect(drawer).toBeVisible();
  return drawer;
}

/**
 * Whether `locator`'s text comes to match `phrase` within `ms`, read here rather than by `expect`, so
 * a miss prints nothing the page held: the exit's screens name the agent, the instrument and the
 * quantity, and a failed run's log is uploaded (`AGENTS.md`, no positions in logs).
 */
async function comesToSay(locator: Locator, phrase: RegExp, ms: number): Promise<boolean> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (phrase.test((await locator.first().textContent({ timeout: 1_000 }).catch(() => null)) ?? "")) return true;
    await locator.page().waitForTimeout(250);
  }
  return false;
}

const reached: Record<string, string> = {};
const write = (extra: Record<string, string> = {}) => writeFileSync(RESULT, JSON.stringify({ ...reached, ...extra }));
const step = (name: string, value = "ok") => {
  reached[name] = value;
  write();
};
/** Records why the loop stopped, then fails, so the trace of what the page did is kept. */
const refuse = (error: string): never => {
  write({ error });
  throw new Error(error);
};

/** How long a screen is watched in place, its own polling running, before it is loaded again. */
const WATCH_MS = 15_000;

/**
 * Opens `path` and waits for `ready` within `ms`, watching the screen in place: the agent's screen
 * and the others poll the API themselves, so a reload would only throw away a read still on its way
 * (the 2026-10-27 window's step 4: a first render slower than one reload cycle was never seen). The
 * screen is loaded again only after `WATCH_MS` with nothing, for a screen that stopped polling.
 */
async function until(page: Page, path: string, ms: number, ready: () => Promise<boolean>): Promise<boolean> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    await page.goto(path, { timeout: 15_000 });
    const reload = Math.min(deadline, Date.now() + WATCH_MS);
    while (Date.now() < reload) {
      if (await ready()) return true;
      await page.waitForTimeout(500);
    }
  }
  return ready();
}

test("the trade loop from web/: approve in the agent's thread, hear of the fill, exit from Details, and kill the agent", async ({ page }) => {
  test.setTimeout(PROPOSAL_MS + CONFIRM_MS + 2 * FILL_MS + 120_000);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE, AGENT, CONFIRM_REQUEST, CONFIRM_DONE })) expect(value, `the driver names ${name}`).not.toBe("");

  await test.step("the agent asks for the owner's OK in its thread, with the checks its dry run deferred", async () => {
    let approval = "";
    const contents = new Map<string, Record<string, unknown>>();
    page.on("response", async (r) => {
      const url = new URL(r.url());
      if (r.request().method() !== "GET" || !r.ok()) return;
      const detail = url.pathname.match(new RegExp(`^/v1/workspaces/${WORKSPACE}/approvals/([^/]+)$`));
      if (detail) {
        const content = ((await r.json().catch(() => ({}))) as { approval?: { content?: Record<string, unknown> } }).approval?.content;
        if (content) contents.set(decodeURIComponent(detail[1]), content);
        return;
      }
      if (url.pathname !== `/v1/workspaces/${WORKSPACE}/agents/${AGENT}/messages`) return;
      const read = (await r.json().catch(() => ({}))) as { messages?: Array<{ card?: { kind?: string; approval_id?: string } | null }> };
      approval ||= read.messages?.map((m) => m.card).find((c) => c?.kind === "approval")?.approval_id ?? "";
    });
    const shown = await until(page, `/agents/${AGENT}`, PROPOSAL_MS, async () => approval !== "" && (await cards(page, "approval").count()) > 0);
    if (!shown) refuse(`agent ${AGENT} asked for no OK in its thread within ${PROPOSAL_MS} ms`);
    step("approval", approval);
    await cards(page, "approval").last().getByRole("link", { name: "Review and answer" }).click();
    await expect(page).toHaveURL(new RegExp(`/approvals/${approval}$`));

    await expect.poll(() => contents.has(approval), { message: "the API's read of the request", timeout: 30_000 }).toBe(true);
    const deferred = contents.get(approval)?.deferred_to_binding;
    const region = page.getByRole("region", { name: "Checks not run yet" });
    if (!Array.isArray(deferred) || deferred.length === 0 || !deferred.every((n) => typeof n === "string")) {
      refuse(`the API served the cell's ask without the checks its dry run deferred (DEC-2388 item 13): ${JSON.stringify(deferred ?? null)}`);
    } else {
      await expect(region).toBeVisible({ timeout: 15_000 });
      await expect(region.getByText(APPROVER_SENTENCE, { exact: true })).toBeVisible();
      await expect(region.getByRole("listitem")).toHaveText(deferred.map(deferredCheckLabel));
      await expect(region).not.toContainText(/passed|cleared/i);
      step("deferred_checks", deferred.join(","));
    }
  });

  await test.step("the owner approves with the code the host confirms", async () => {
    let challenge = { challenge_id: "" };
    await page.route(`**/v1/workspaces/${WORKSPACE}/step-up/challenges`, async (route) => {
      const response = await route.fetch();
      challenge = (await response.json()) as typeof challenge;
      await route.fulfill({ response });
    });
    let principal = "";
    await page.route("**/v1/me/session", async (route) => {
      const response = await route.fetch();
      principal = ((await response.json()) as { principal_id?: string }).principal_id ?? "";
      await route.fulfill({ response });
    });
    await page.getByRole("button", { name: "Approve", exact: true }).click();
    const panel = page.getByRole("region", { name: "Confirm on the host" });
    await expect(panel).toBeVisible();
    const code = ((await panel.locator("[data-slot=host-confirm-code]").textContent()) ?? "").trim();
    if (!challenge.challenge_id || !principal) refuse("the page showed a code without a challenge or a principal the API answered");
    step("challenge", challenge.challenge_id);
    writeFileSync(CONFIRM_REQUEST, JSON.stringify({ workspace: WORKSPACE, principal, agent: AGENT, approval: reached.approval, challenge: challenge.challenge_id, code }));
    const deadline = Date.now() + CONFIRM_MS;
    while (!existsSync(CONFIRM_DONE) && Date.now() < deadline) await page.waitForTimeout(500);
    if (!existsSync(CONFIRM_DONE)) refuse(`the host did not confirm the code within ${CONFIRM_MS} ms`);
    const done = JSON.parse(readFileSync(CONFIRM_DONE, "utf8")) as { ok?: boolean; error?: string };
    if (!done.ok) refuse(`the host refused the confirmation: ${done.error ?? "no reason given"}`);
    const answered = page.waitForResponse((r) => r.request().method() === "POST" && /\/approvals\/[^/]+\/responses$/.test(new URL(r.url()).pathname));
    await panel.getByRole("button", { name: "Approve", exact: true }).click();
    const answer = await answered;
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the approve: ${(await answer.text()).slice(0, 400)}`);
    await expect(page.getByRole("status").filter({ hasText: /Recorded in the journal: you approved|Approved: the gate re-ran/ })).toBeVisible({ timeout: 30_000 });
    step("approved", ((await answer.json()) as { response_id?: string }).response_id ?? "");
  });

  await test.step("the agent says the fill in its thread, and its Details show the position and the filled order", async () => {
    const filled = await until(page, `/agents/${AGENT}`, FILL_MS, async () => (await cards(page, "fill").count()) > 0);
    if (!filled) refuse(`the agent said no fill in its thread within ${FILL_MS} ms`);
    step("order_filled");
    const positions = page.getByRole("region", { name: "Positions", exact: true }).getByRole("listitem");
    const activity = page.getByRole("region", { name: "Activity", exact: true }).getByRole("listitem");
    const deadline = Date.now() + 30_000;
    let held = false;
    while (!held && Date.now() < deadline) {
      await openDetails(page);
      held = await expect
        .poll(async () => (await positions.count()) > 0 && (await activity.filter({ hasText: /· filled$/ }).count()) > 0, { timeout: 10_000 })
        .toBe(true)
        .then(() => true, () => false);
    }
    if (!held) {
      /** What a section shows, never what it lists: the uploaded failure carries no position or order (`AGENTS.md`, no positions in logs). */
      const showed = async (title: string) => {
        const region = page.getByRole("region", { name: title, exact: true });
        if ((await region.count()) === 0) return "absent";
        const listed = await region.first().getByRole("listitem").count();
        if (listed > 0) return `${listed} listed`;
        const text = (await region.first().textContent({ timeout: 2_000 }).catch(() => null)) ?? "";
        return SECTION_STATES.find((state) => text.includes(state)) ?? "an unrecognized state";
      };
      refuse(`the agent said the fill but its Details showed no position or no filled order within 30 s; positions: ${await showed("Positions")}; activity: ${await showed("Activity")}`);
    }
    step("position");
    step("activity", String(await activity.count()));
  });

  await test.step("the owner's exit from Details is recorded, and the agent says its sell in the thread", async () => {
    await page.goto(`/agents/${AGENT}`, { timeout: 15_000 });
    const before = await cards(page, "fill").filter({ hasText: /^Filled\s*Sell / }).count();
    const drawer = await openDetails(page);
    const exit = drawer.getByRole("button", { name: /^Exit / }).first();
    await expect(exit).toBeVisible({ timeout: 30_000 });
    step("details");
    await exit.click();
    const confirm = drawer.getByRole("group", { name: /^Exit / });
    if (!(await comesToSay(confirm, /only participation caps pace it/, 15_000))) refuse("the owner exit's confirmation did not say that only participation caps pace it within 15 s");
    const sent = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/agents/${AGENT}/exits`);
    await confirm.getByRole("button", { name: "Sell it" }).click();
    const answer = await sent;
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the owner exit: ${(await answer.text()).slice(0, 400)}`);
    const exitStatus = drawer.getByRole("status", { name: /^Your exit from / });
    if (!(await comesToSay(exitStatus, /Recorded in the journal: your exit|Taken by the agent's runtime/, 30_000))) refuse("the owner exit's status did not read recorded or taken within 30 s");
    step("owner_exit", ((await answer.json()) as { command_id?: string }).command_id ?? "");
    await page.keyboard.press("Escape");
    const said = await until(page, `/agents/${AGENT}`, FILL_MS, async () => (await cards(page, "fill").filter({ hasText: /^Filled\s*Sell / }).count()) > before);
    if (!said) refuse(`the owner exit was recorded but the agent said no sell filled within ${FILL_MS} ms`);
    step("exit_filled");
  });

  await test.step("the browser's kill switch is recorded for this agent alone", async () => {
    await page.goto(`/agents/${AGENT}`, { timeout: 15_000 });
    await page.getByRole("button", { name: "Stop", exact: true }).first().click();
    const sheet = page.getByRole("dialog", { name: /^Stop/ });
    await expect(sheet).toBeVisible();
    await sheet.getByRole("link", { name: /^Kill switch:/ }).first().click();
    await expect(page).toHaveURL(new RegExp(`/agents/${AGENT}/kill-switch$`));
    let scope: unknown = null;
    let wrongScope = false;
    await page.route(`**/v1/workspaces/${WORKSPACE}/kill-switch`, async (route) => {
      scope = (JSON.parse(route.request().postData() ?? "{}") as { scope?: unknown }).scope ?? null;
      wrongScope = JSON.stringify(scope) !== JSON.stringify({ kind: "agent", id: AGENT });
      await (wrongScope ? route.abort() : route.continue());
    });
    const recorded = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/kill-switch`);
    await page.getByRole("button", { name: /Activate the kill switch/ }).click();
    await page.getByRole("dialog", { name: "Confirm the kill switch" }).getByRole("button", { name: "Activate" }).click();
    const answer = await Promise.race([recorded, page.waitForEvent("requestfailed", { predicate: () => wrongScope, timeout: 30_000 }).then(() => null)]);
    if (answer === null) return refuse(`the kill switch was sent for ${JSON.stringify(scope)}, not agent ${AGENT} alone, and was stopped`);
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the kill switch`);
    step("kill_switch", ((await answer.json()) as { command_id?: string }).command_id ?? "");
  });

  await test.step("after the kill switch, the agent reads as stopped", async () => {
    const stopped = await until(page, `/agents/${AGENT}`, 30_000, async () => (await page.getByText("Stopped: questions only").count()) > 0);
    if (!stopped) refuse("the agent did not read as stopped within 30 s of its kill switch");
    step("stopped_shown");
  });
});
