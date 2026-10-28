import { writeFileSync } from "node:fs";
import { type Page, type Response, expect, test } from "@playwright/test";

/**
 * The demo's step 5 from web/, against a running local cell on paper (lane W): the owner, signed in
 * through the stub issuer, opens the backtest's report from its link, `/backtests/{id}`, since the
 * API lists no backtests, and sees the report the runner stored, read through the API
 * (`GET /backtests/{id}`, DEC-2400): completed, its comparison with buy and hold drawn, and nothing
 * labelled as fixture data.
 *
 * The driver (`cargo xtask cell demo`, step 5) names the workspace and the backtest, after the Job
 * has run and recorded it, and reads the steps this writes to `OWLHEAD_CELL_RESULT`, so a failure
 * names the first step that did not happen. It needs no market session, so it runs in or out of
 * the trade loop's window. Paper only: nothing here reaches a broker.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const WORKSPACE = process.env.OWLHEAD_WORKSPACE_ID ?? "";
const BACKTEST = process.env.OWLHEAD_CELL_BACKTEST ?? "";
/** The whole test's bound, and one page's: `next dev` compiles the route on its first visit. */
const REPORT_MS = Number(process.env.OWLHEAD_CELL_REPORT_MS ?? 180_000);
const PAGE_MS = 90_000;

const reached: Record<string, string> = {};
const write = (extra: Record<string, string> = {}) => writeFileSync(RESULT, JSON.stringify({ ...reached, ...extra }));
const step = (name: string, value = "ok") => {
  reached[name] = value;
  write();
};
/** Records why the visit stopped, then fails, so the trace of what the page did is kept. */
const refuse = (error: string): never => {
  write({ error });
  throw new Error(error);
};

/**
 * Opens `path` until its read of `apiPath` is not the one 401: `web/` opens the API session through
 * the stub issuer on its first load, so the first read may run before the cookie is set. Every pass
 * shares one `PAGE_MS` budget; answers the last response, so a failure names what the API said.
 */
async function signedInRead(page: Page, path: string, apiPath: string): Promise<Response> {
  const deadline = Date.now() + PAGE_MS;
  for (;;) {
    const left = Math.max(deadline - Date.now(), 1_000);
    const read = page.waitForResponse((r) => r.request().method() === "GET" && new URL(r.url()).pathname === apiPath, { timeout: left }).catch(() => null);
    await page.goto(path, { timeout: left });
    const answer = await read;
    if (!answer) return refuse(`web/ never read ${apiPath}`);
    if (answer.status() !== 401 || deadline - Date.now() < 5_000) return answer;
    await page.waitForTimeout(2_000);
  }
}

test("the backtest's report from its link, read through the API", async ({ page }) => {
  test.setTimeout(REPORT_MS);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE, BACKTEST })) expect(value, `the driver names ${name}`).not.toBe("");

  const link = `/backtests/${encodeURIComponent(BACKTEST)}`;
  await test.step("the owner opens the report's link, and web/ reads it through the API", async () => {
    const response = await signedInRead(page, link, `/v1/workspaces/${WORKSPACE}/backtests/${BACKTEST}`);
    if (response.status() !== 200) refuse(`the API answered ${response.status()} for the backtest: ${(await response.text().catch(() => "")).slice(0, 400)}`);
    const read = (await response.json()) as { backtest?: { state?: string } };
    if (read.backtest?.state !== "completed") refuse(`the API served the backtest as ${read.backtest?.state ?? "no state"}, not completed`);
    step("report_link", link);
  });

  await test.step("the report the runner stored is shown, with nothing labelled as fixture data", async () => {
    const compared = page.getByRole("table", { name: "This version and buy and hold" });
    try {
      await expect(compared).toBeVisible({ timeout: 30_000 });
    } catch {
      refuse(`the report screen showed no comparison with buy and hold; it said: ${((await page.getByRole("main").textContent().catch(() => "")) ?? "").replace(/\s+/g, " ").slice(0, 300)}`);
    }
    if ((await page.locator("[data-slot=fixture-tag]").count()) > 0) refuse("the report screen labelled its figures as fixture data");
    step("report_shown");
  });
});
