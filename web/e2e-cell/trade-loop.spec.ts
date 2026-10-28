import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { type Page, expect, test } from "@playwright/test";
import { deferredCheckLabel } from "../src/lib/deferred-checks";

/**
 * The trade loop from web/, end to end in the local cell (the founder's 2026-10-27 deadline): a
 * deployed paper agent's request appears in the web app, the owner approves it in the browser with
 * the code the host confirms (`cli_confirm`, DEC-2104), the executor places and fills the order on
 * the fixture broker, the agent's screen shows the order filled, the position and the journal
 * records from the served reads (DEC-2377), the positions screen shows the agent's own position, the
 * audit timeline shows the agent's and its account's records, the owner pauses the agent from its
 * page (`POST /agents/{id}/pause`, no step-up) and it reads paused, the browser's kill switch is
 * recorded for the paused agent, which a pause leaves available (trading spec §7.4), and the alerts screen shows that kill switch as exactly one alert (`GET /alerts`, DEC-2141).
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

test("the trade loop from web/: approve in the browser, see the fill, the position and the journal, pause the agent, and kill it", async ({ page }) => {
  test.setTimeout(PROPOSAL_MS + CONFIRM_MS + 2 * FILL_MS + 120_000);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE, AGENT, CONFIRM_REQUEST, CONFIRM_DONE })) expect(value, `the driver names ${name}`).not.toBe("");

  await test.step("the agent's request shows in the web app, with the checks its dry run deferred", async () => {
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
      if (url.pathname !== `/v1/workspaces/${WORKSPACE}/approvals` || url.searchParams.get("state") !== "pending") return;
      const listed = ((await r.json().catch(() => ({}))) as { approvals?: Array<{ approval_id: string; agent_id: string }> }).approvals ?? [];
      approval ||= listed.find((a) => a.agent_id === AGENT)?.approval_id ?? "";
    });
    const shown = await until(page, "/approvals", PROPOSAL_MS, async () => approval !== "");
    if (!shown) refuse(`no request from agent ${AGENT} waited in the web app within ${PROPOSAL_MS} ms`);
    step("approval", approval);
    await page.goto(`/approvals/${approval}`, { timeout: 15_000 });

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

  await test.step("the agent's screen shows the order filled, the position, and the journal's records", async () => {
    const orders = page.getByRole("list", { name: "Orders" });
    const positions = page.getByRole("list", { name: "This agent's positions" }).getByRole("listitem");
    const journal = page.getByRole("list", { name: "Journal" });
    const filled = await until(page, `/agents/${AGENT}`, FILL_MS, async () => (await orders.locator("[data-state=filled]").count()) > 0);
    if (!filled) refuse(`no filled order on the agent's screen within ${FILL_MS} ms`);
    step("order_filled");
    const reads: string[] = [];
    page.on("requestfinished", async (request) => {
      const path = new URL(request.url()).pathname;
      if (request.method() !== "GET" || !(path.endsWith(`/agents/${AGENT}/positions`) || path.includes("/journal/streams/agent"))) return;
      const status = (await request.response().catch(() => null))?.status() ?? 0;
      reads.push(`${status} ${path.split("/").slice(-2).join("/")} ${Math.round(request.timing().responseEnd)}ms`);
    });
    const held = await until(page, `/agents/${AGENT}`, 30_000, async () => (await positions.count()) > 0 && (await journal.getByRole("listitem").count()) > 0);
    if (!held) {
      const said = async (title: string) => {
        const region = page.getByRole("region", { name: title, exact: true });
        if ((await region.count()) === 0) return "absent";
        return ((await region.first().textContent({ timeout: 2_000 }).catch(() => null)) ?? "unreadable").replace(/\s+/g, " ").slice(0, 240);
      };
      refuse(`the agent's screen showed the fill but no position or no journal record within 30 s, watched in place; positions: "${await said("This agent's positions")}"; journal: "${await said("Journal")}"; reads (status, route, ms): ${reads.slice(-6).join(", ") || "none"}`);
    }
    step("position");
    step("journal", String(await journal.getByRole("listitem").count()));
  });

  await test.step("the positions screen shows the agent's own position from the API, in its columns", async () => {
    const own = page.locator(`[data-slot=agent-positions][data-agent="${AGENT}"]`);
    const shown = await until(page, "/positions", 30_000, async () => (await own.getByRole("row").count()) > 1);
    if (!shown) refuse("the positions screen showed no row of the agent's own position within 30 s");
    await expect(own.getByRole("columnheader", { name: "Cost basis" })).toBeVisible();
    await expect(own.getByRole("row").nth(1).getByRole("cell")).toHaveCount(8);
    step("positions_screen");
  });

  await test.step("the owner's exit from the positions screen is recorded, and its sell fills on the agent's orders", async () => {
    const sells = page.getByRole("list", { name: "Orders" }).locator("[data-state=filled]").filter({ hasText: /^Sell / });
    await page.goto(`/agents/${AGENT}`, { timeout: 15_000 });
    await page.getByRole("list", { name: "Orders" }).waitFor({ timeout: 30_000 });
    const before = new Set(await sells.allTextContents());
    await page.goto("/positions", { timeout: 15_000 });
    const own = page.locator(`[data-slot=agent-positions][data-agent="${AGENT}"]`);
    const exit = own.getByRole("button", { name: /^Exit / }).first();
    await expect(exit).toBeVisible({ timeout: 30_000 });
    await exit.click();
    const confirm = own.getByRole("group", { name: /^Exit / });
    await expect(confirm).toContainText("only participation caps pace it");
    const sent = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/agents/${AGENT}/exits`);
    await confirm.getByRole("button", { name: "Sell it" }).click();
    const answer = await sent;
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the owner exit: ${(await answer.text()).slice(0, 400)}`);
    await expect(page.getByRole("status", { name: /^Your exit from / })).toContainText(/Recorded in the journal: your exit|Taken by the agent's runtime/, { timeout: 30_000 });
    const filled = await until(page, `/agents/${AGENT}`, FILL_MS, async () => (await sells.allTextContents()).some((sell) => !before.has(sell)));
    if (!filled) refuse(`the owner exit was recorded but no new sell filled on the agent's orders within ${FILL_MS} ms`);
    step("owner_exit", ((await answer.json()) as { command_id?: string }).command_id ?? "");
  });

  await test.step("the audit timeline shows the agent's records and its account's, as the journal holds them", async () => {
    const records = page.getByRole("list", { name: "Journal records" });
    const agent = records.locator(`[data-stream="agent:${WORKSPACE}:${AGENT}"]`);
    const account = records.locator('[data-stream^="acct:"]');
    const shown = await until(page, "/audit/timeline", 30_000, async () => (await agent.count()) > 0 && (await account.count()) > 0);
    if (!shown) refuse("the audit timeline showed no record of the agent's stream and of an account stream within 30 s");
    step("audit_timeline");
  });

  await test.step("the owner pauses the agent from its page, and the agent reads paused", async () => {
    await page.goto(`/agents/${AGENT}`, { timeout: 15_000 });
    const header = page.locator("article header").first();
    const pause = header.getByRole("button", { name: "Pause", exact: true });
    await expect(pause).toBeVisible({ timeout: 30_000 });
    const sent = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/agents/${AGENT}/pause`);
    await pause.click();
    const answer = await sent;
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the pause: ${(await answer.text()).slice(0, 400)}`);
    await expect(header.getByRole("status").filter({ hasText: /^Pause / })).toHaveText("Pause recorded in the journal.", { timeout: 30_000 });
    const paused = header.locator('[data-slot="mode-badge"][data-mode="paused"]');
    if (!(await until(page, `/agents/${AGENT}`, 30_000, async () => (await paused.count()) > 0))) refuse("the pause was recorded but the agent's page did not read it paused within 30 s");
    step("paused", ((await answer.json()) as { command_id?: string }).command_id ?? "");
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

  await test.step("the alerts screen shows the kill switch as exactly one alert", async () => {
    const killed = page.locator('[data-slot="api-alert"][data-kind="kill_switch"]');
    const shown = await until(page, "/alerts", 30_000, async () => (await killed.count()) > 0);
    if (!shown) refuse("the alerts screen showed no kill switch alert within 30 s of the kill switch");
    const count = await killed.count();
    if (count !== 1) refuse(`the alerts screen showed ${count} kill switch alerts for one kill switch, not one`);
    await expect(killed.getByText("Kill switch activated")).toBeVisible();
    step("kill_switch_alert");
  });
});
