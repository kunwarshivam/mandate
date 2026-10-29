import { writeFileSync } from "node:fs";
import { type Page, type Response, expect, test } from "@playwright/test";

/**
 * The demo's step 5 from web/, against a running local cell on paper (lane W): the owner, signed in
 * through the stub issuer on web/'s Home, reads the backtest's report with that session through the
 * API (`GET /backtests/{id}`, DEC-2400), since web/ has no report screen (DEC-2293: Home, the
 * agent's thread and Settings): completed, with its comparison with buy and hold.
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

test("the backtest's report, read through the API with the owner's web session", async ({ page }) => {
  test.setTimeout(REPORT_MS);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE, BACKTEST })) expect(value, `the driver names ${name}`).not.toBe("");

  const route = `/v1/workspaces/${WORKSPACE}/backtests/${encodeURIComponent(BACKTEST)}`;
  await test.step("the owner, signed in on Home, reads the backtest through the API", async () => {
    const home = await signedInRead(page, "/", `/v1/workspaces/${WORKSPACE}/home`);
    if (!home.ok()) refuse(`the API answered ${home.status()} for Home`);
    const read = await page.evaluate(
      async (url) => {
        const response = await fetch(url, { credentials: "include", headers: { Accept: "application/json" } });
        return { status: response.status, body: await response.text() };
      },
      `${new URL(home.url()).origin}${route}`,
    );
    if (read.status !== 200) refuse(`the API answered ${read.status} for the backtest: ${read.body.slice(0, 400)}`);
    const served = JSON.parse(read.body) as { backtest?: { state?: string; report?: { benchmark?: unknown; equity?: { benchmark?: unknown[] } } | null } };
    if (served.backtest?.state !== "completed") refuse(`the API served the backtest as ${served.backtest?.state ?? "no state"}, not completed`);
    step("report_read", BACKTEST);
    const report = served.backtest?.report;
    if (!report?.benchmark || !Array.isArray(report.equity?.benchmark) || report.equity.benchmark.length === 0) refuse("the stored report holds no comparison with buy and hold");
    step("report_compared");
  });
});
