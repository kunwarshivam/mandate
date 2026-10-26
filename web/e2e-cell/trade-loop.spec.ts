import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { type Page, expect, test } from "@playwright/test";

/**
 * The trade loop from web/, end to end in the local cell (the founder's 2026-10-27 deadline): a
 * deployed paper agent's request appears in the web app, the owner approves it in the browser with
 * the code the host confirms (`cli_confirm`, DEC-2104), the executor places and fills the order on
 * the fixture broker, the agent's screen shows the order filled, the position and the journal
 * records from the served reads (DEC-2377), and the browser's kill switch is recorded.
 *
 * The driver sets it up and plays the host. It runs this file with `OWLHEAD_CELL_SPEC=trade-loop.spec.ts`
 * and the agent in `OWLHEAD_CELL_AGENT`. When the page shows a code, the spec writes
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

/** Reloads `path` until `locator` shows, within `ms`: the screens read on open and poll, but a reload is the plain way to wait across them. */
async function until(page: Page, path: string, ms: number, ready: () => Promise<boolean>): Promise<boolean> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    await page.goto(path);
    try {
      await page.waitForLoadState("networkidle", { timeout: 10_000 });
    } catch {
      // fallthrough: a screen that keeps polling never goes idle
    }
    if (await ready()) return true;
    await page.waitForTimeout(3_000);
  }
  return false;
}

test("the trade loop from web/: approve in the browser, see the fill, the position and the journal, and kill the agent", async ({ page }) => {
  test.setTimeout(PROPOSAL_MS + CONFIRM_MS + FILL_MS + 120_000);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE, AGENT, CONFIRM_REQUEST, CONFIRM_DONE })) expect(value, `the driver names ${name}`).not.toBe("");

  await test.step("the agent's request shows in the web app", async () => {
    const shown = await until(page, "/approvals", PROPOSAL_MS, async () => (await page.getByRole("region", { name: "Waiting" }).getByRole("link").count()) > 0);
    if (!shown) refuse(`no request waited in the web app within ${PROPOSAL_MS} ms`);
    const link = page.getByRole("region", { name: "Waiting" }).getByRole("link").first();
    const href = (await link.getAttribute("href")) ?? "";
    step("approval", href.split("/").at(-1) ?? "");
    await link.click();
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
    await expect(page.getByRole("status").filter({ hasText: "Recorded in the journal: you approved" })).toBeVisible();
    step("approved", ((await answer.json()) as { response_id?: string }).response_id ?? "");
  });

  await test.step("the agent's screen shows the order filled, the position, and the journal's records", async () => {
    const filled = await until(page, `/agents/${AGENT}`, FILL_MS, async () => (await page.getByRole("list", { name: "Orders" }).locator("[data-state=filled]").count()) > 0);
    if (!filled) refuse(`no filled order on the agent's screen within ${FILL_MS} ms`);
    step("order_filled");
    await expect(page.getByRole("list", { name: "This agent's positions" }).getByRole("listitem")).not.toHaveCount(0);
    step("position");
    const journal = page.getByRole("list", { name: "Journal" });
    await expect(journal).toBeVisible();
    step("journal", String(await journal.getByRole("listitem").count()));
  });

  await test.step("the browser's kill switch is recorded for this agent alone", async () => {
    await page.getByRole("button", { name: "Stop", exact: true }).first().click();
    const sheet = page.getByRole("dialog", { name: /^Stop/ });
    await expect(sheet).toBeVisible();
    await sheet.getByRole("link", { name: /^Kill switch:/ }).first().click();
    await expect(page).toHaveURL(new RegExp(`/agents/${AGENT}/kill-switch$`));
    let scope: unknown = null;
    await page.route(`**/v1/workspaces/${WORKSPACE}/kill-switch`, async (route) => {
      scope = (JSON.parse(route.request().postData() ?? "{}") as { scope?: unknown }).scope ?? null;
      await (JSON.stringify(scope) === JSON.stringify({ kind: "agent", id: AGENT }) ? route.continue() : route.abort());
    });
    const recorded = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/kill-switch`);
    await page.getByRole("button", { name: /Activate the kill switch/ }).click();
    const passkey = page.getByRole("dialog", { name: "Confirm it is you" });
    const first = await Promise.race([recorded.then(() => "sent" as const), passkey.waitFor({ state: "visible" }).then(() => "passkey" as const)]);
    if (first === "passkey") await passkey.getByRole("button", { name: "Use passkey" }).click();
    const answer = await recorded;
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the kill switch`);
    step("kill_switch", ((await answer.json()) as { command_id?: string }).command_id ?? "");
  });
});
