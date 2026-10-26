import { writeFileSync } from "node:fs";
import { type Page, expect, test } from "@playwright/test";

/**
 * The customer's journey from web/, against a running local cell on Alpaca paper (lane GL,
 * DEC-2134): after the demo scenario has signed the owner in, connected Alpaca paper, confirmed and
 * deployed the agent, and run the trade loop, the owner, signed in through the stub issuer:
 *
 * 1. sees the workspace on paper with its Alpaca paper connection, and Robinhood shown as real money,
 *    off;
 * 2. reads the go-live screen: every risk disclosure and platform limit marked pending counsel, and
 *    the disclosure's acceptance, which the driver made on the host first, read back from the
 *    journal;
 * 3. asks to trade real money for the agent, and is refused: real money stays off for a customer
 *    workspace until counsel signs off;
 * 4. finds that refusal journaled as `RealMoneyRefused` on the workspace's control stream, read
 *    through the API's audit read (whose bodies are base64).
 *
 * The driver (`cargo xtask cell journey`) names the workspace and the agent and reads the steps this
 * writes to `OWLHEAD_CELL_RESULT`, so a failure names the first step that did not happen. Paper only:
 * no step here can reach a live venue, and the one request that names real money is refused.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const WORKSPACE = process.env.OWLHEAD_WORKSPACE_ID ?? "";
const AGENT = process.env.OWLHEAD_CELL_AGENT ?? "";
const API = process.env.OWLHEAD_API_ORIGIN ?? "";
const PENDING_COUNSEL = "Pending counsel — not for real-money use";
/** The whole test's bound, and one page's: `next dev` compiles each route on its first visit. */
const JOURNEY_MS = Number(process.env.OWLHEAD_CELL_JOURNEY_MS ?? 240_000);
const PAGE_MS = 90_000;

const reached: Record<string, string> = {};
const write = (extra: Record<string, string> = {}) => writeFileSync(RESULT, JSON.stringify({ ...reached, ...extra }));
const step = (name: string, value = "ok") => {
  reached[name] = value;
  write();
};
const refuse = (error: string): never => {
  write({ error });
  throw new Error(error);
};

/**
 * Opens `path` until its read of `apiPath` is not the one 401: `web/` opens the API session through
 * the stub issuer on its first load, so the first reads may run before the cookie is set. Answers
 * the last response, so a failure names what the API said.
 */
async function signedInRead(page: Page, path: string, apiPath: string) {
  const deadline = Date.now() + PAGE_MS;
  for (;;) {
    const read = page.waitForResponse((r) => r.request().method() === "GET" && new URL(r.url()).pathname === apiPath, { timeout: PAGE_MS }).catch(() => null);
    await page.goto(path, { timeout: PAGE_MS });
    const answer = await read;
    if (!answer) return refuse(`web/ never read ${apiPath}`);
    if (answer.status() !== 401 || Date.now() > deadline) return answer;
    await page.waitForTimeout(2_000);
  }
}

/** The control stream's events of `type`, read through the API's audit read with the page's session. */
async function controlEvents(page: Page, type: string): Promise<Array<{ event_id: string; payload: Record<string, unknown> }>> {
  return page.evaluate(
    async ({ api, workspace, type }) => {
      const found: Array<{ event_id: string; payload: Record<string, unknown> }> = [];
      let after = 0;
      for (let pages = 0; pages < 50; pages += 1) {
        const url = `${api}/v1/workspaces/${workspace}/journal/streams/${encodeURIComponent(`ctl:${workspace}`)}/events?after_seq=${after}&limit=1000`;
        const response = await fetch(url, { credentials: "include", headers: { Accept: "application/json" } });
        if (!response.ok) throw new Error(`the audit read answered ${response.status}`);
        const page = (await response.json()) as { events: Array<{ event_id: string; event_type: string; body: string }>; next_after_seq: number; at_head: boolean };
        for (const event of page.events) {
          if (event.event_type !== type) continue;
          const bytes = Uint8Array.from(atob(event.body), (c) => c.charCodeAt(0));
          const body = JSON.parse(new TextDecoder().decode(bytes)) as { payload: Record<string, unknown> };
          found.push({ event_id: event.event_id, payload: body.payload });
        }
        if (page.at_head) break;
        after = page.next_after_seq;
      }
      return found;
    },
    { api: API, workspace: WORKSPACE, type },
  );
}

test("the customer's journey on Alpaca paper: connections, going live, and real money refused and journaled", async ({ page }) => {
  test.setTimeout(JOURNEY_MS);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE, AGENT, API })) expect(value, `the driver names ${name}`).not.toBe("");

  await test.step("the workspace is on paper, with Alpaca paper connected and Robinhood off", async () => {
    const answer = await signedInRead(page, "/connections", `/v1/workspaces/${WORKSPACE}/connections`);
    const body = (await answer.text().catch(() => "")).slice(0, 600);
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the connections: ${body}`);
    const accounts = page.getByRole("list", { name: "Broker accounts" });
    const shown = await accounts.waitFor({ state: "visible", timeout: 30_000 }).then(() => true, () => false);
    if (!shown) refuse(`web/ did not list the connections the API answered: ${body}`);
    if ((await accounts.getByRole("link", { name: "Alpaca" }).count()) === 0) refuse("no Alpaca connection is listed");
    await expect(page.locator("[data-slot=robinhood-real-money]")).toContainText("Real money: off.");
    step("connections");
  });

  await test.step("the go-live screen marks every disclosure and limit pending counsel", async () => {
    await page.goto("/go-live", { timeout: PAGE_MS });
    const disclosures = page.getByRole("region", { name: "Risks of an automated agent" });
    await expect(disclosures).toBeVisible({ timeout: PAGE_MS });
    await expect(disclosures.getByText(PENDING_COUNSEL)).toBeVisible();
    await expect(page.getByRole("region", { name: "Platform limits" }).getByText(PENDING_COUNSEL)).toBeVisible();
    step("disclosures", String(await disclosures.getByRole("article").count()));
    const acceptance = page.getByRole("region", { name: "Your acceptance" }).getByRole("status");
    await expect(acceptance).toHaveAttribute("data-state", "accepted", { timeout: 30_000 });
    const principal = await page.evaluate(async (api) => {
      const response = await fetch(`${api}/v1/me/session`, { credentials: "include", headers: { Accept: "application/json" } });
      return response.ok ? ((await response.json()) as { principal_id?: string }).principal_id ?? "" : "";
    }, API);
    const accepted = (await controlEvents(page, "DisclosureAccepted")).filter((e) => e.payload.document === "go_live_risk" && String(e.payload.user).toUpperCase() === principal.toUpperCase());
    const last = accepted.at(-1);
    const method = (last?.payload.step_up as { method?: string } | null | undefined)?.method;
    if (!last || method !== "cli_confirm") refuse(`no DisclosureAccepted of go_live_risk by ${principal || "the owner"} with cli_confirm step-up`);
    step("disclosure_accepted", last?.event_id ?? "");
  });

  await test.step("asking for real money is refused, and the agent stays on paper", async () => {
    const before = (await controlEvents(page, "RealMoneyRefused")).length;
    const region = page.getByRole("region", { name: "Real money" });
    await region.getByRole("combobox", { name: "Agent" }).selectOption(AGENT);
    const answered = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/agents/${AGENT}/go-live`);
    await region.getByRole("button", { name: "Ask to trade real money" }).click();
    const answer = await answered;
    if (answer.status() !== 409) refuse(`the API answered ${answer.status()} to a real-money request, not 409`);
    const problem = (await answer.json()) as { code?: string };
    if (problem.code !== "live_unavailable") refuse(`the API refused real money as ${problem.code ?? "nothing"}, not live_unavailable`);
    await expect(region.getByRole("status")).toContainText("real-money trading is off for your workspace until counsel signs off");
    step("real_money_refused");

    const after = await controlEvents(page, "RealMoneyRefused");
    if (after.length !== before + 1) refuse(`the control stream holds ${after.length} RealMoneyRefused, not ${before + 1}`);
    const refusal = after[after.length - 1];
    if (refusal.payload.action !== "go_live" || refusal.payload.reason !== "gate_off") refuse(`the refusal records ${JSON.stringify(refusal.payload)}`);
    step("refusal_journaled", refusal.event_id);
  });
});
