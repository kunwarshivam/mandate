import { writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

/**
 * The cell demo's step 6 from web/ (lane W2): the owner, signed in through the stub issuer, opens
 * Stop and presses one agent's kill switch, and the API records it. It writes the workspace and the
 * kill switch's command id to `OWLHEAD_CELL_RESULT`, which the scenario checks against the
 * workspace's control stream; with no agent in the workspace it writes the reason and presses
 * nothing, never falling back to a workspace-wide switch the owner did not ask for. A named agent
 * that is already stopped is a reason too: D10 offers nothing to confirm for a stopped agent.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const WORKSPACE = process.env.OWLHEAD_WORKSPACE_ID ?? "";
/** The agent to kill, when the driver names one; otherwise the first the API lists that is not stopped. */
const AGENT = process.env.OWLHEAD_CELL_AGENT ?? "";

const write = (body: Record<string, string>) => writeFileSync(RESULT, JSON.stringify(body));

/**
 * One `GET` the page must make, and its answer, so a failure names what the API said rather than
 * timing out silently. `waitForResponse` on `r.ok()` alone reports only that 30 s passed, which is
 * what made this gesture's first failures unreadable (the tracker's `#975` remedy, one level up).
 */
const GET_TIMEOUT_MS = 20_000;

async function detailAnswer(page: import("@playwright/test").Page, pathname: string, goto: string) {
  let seen: { status: number; body: string } | null = null;
  const answered = page
    .waitForResponse((r) => r.request().method() === "GET" && new URL(r.url()).pathname === pathname, { timeout: GET_TIMEOUT_MS })
    .catch(() => null);
  await page.goto(goto);
  const response = await answered;
  if (response) {
    const body = await response.text().catch(() => "");
    seen = { status: response.status(), body: body.slice(0, 600) };
  }
  return seen;
}

test("the kill switch from web/, recorded through the API", async ({ page }) => {
  expect(RESULT, "the driver names where the result goes").not.toBe("");
  const agents = page.waitForResponse((r) => r.request().method() === "GET" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/agents` && r.ok());
  await page.goto("/agents");
  const listed = (await (await agents).json()) as { agents?: Array<{ agent_id: string; mode: string }> };
  const named = AGENT ? listed.agents?.find((a) => a.agent_id === AGENT) : undefined;
  if (AGENT && !named) return write({ error: `agent ${AGENT} is not in the API's workspace` });
  if (named?.mode === "stopped") return write({ error: `agent ${AGENT} is already stopped; web/'s kill switch screen offers nothing to confirm for a stopped agent` });
  const agent = named ?? listed.agents?.find((a) => a.mode !== "stopped");
  if (!agent) return write({ error: "no agent in the API's workspace to kill" });
  const detailPath = `/v1/workspaces/${WORKSPACE}/agents/${agent.agent_id}`;
  const detail = await detailAnswer(page, detailPath, `/agents/${agent.agent_id}`);
  if (!detail) return write({ error: `web/ made no GET ${detailPath} within ${GET_TIMEOUT_MS} ms` });
  if (detail.status < 200 || detail.status >= 300) return write({ error: `the API answered ${detail.status} for ${detailPath}: ${detail.body}` });
  await expect(page.getByRole("main")).not.toBeEmpty();
  await page.getByRole("button", { name: "Stop", exact: true }).first().click();
  const sheet = page.getByRole("dialog", { name: /^Stop/ });
  await expect(sheet).toBeVisible();
  await sheet.getByRole("link", { name: /^Kill switch:/ }).first().click();
  await expect(page).toHaveURL(new RegExp(`/agents/${agent.agent_id}/kill-switch$`));
  let scopeShown: unknown = null;
  await page.route(`**/v1/workspaces/${WORKSPACE}/kill-switch`, async (route) => {
    scopeShown = (JSON.parse(route.request().postData() ?? "{}") as { scope?: unknown }).scope ?? null;
    const exact = JSON.stringify(scopeShown) === JSON.stringify({ kind: "agent", id: agent.agent_id });
    await (exact ? route.continue() : route.abort());
  });
  const recorded = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/kill-switch`);
  await page.getByRole("button", { name: /Activate the kill switch/ }).click();
  const passkey = page.getByRole("dialog", { name: "Confirm it is you" });
  const first = await Promise.race([recorded.then(() => "sent" as const), passkey.waitFor({ state: "visible" }).then(() => "passkey" as const)]);
  if (first === "passkey") await passkey.getByRole("button", { name: "Use passkey" }).click();
  const answer = await recorded;
  expect(scopeShown, "the request carried exactly this agent's scope before it was let through").toEqual({ kind: "agent", id: agent.agent_id });
  expect(answer.status(), "the API recorded the kill switch").toBe(202);
  const body = (await answer.json()) as { command_id?: string; phase?: string };
  expect(body.phase).toBe("recorded");
  expect(JSON.parse(answer.request().postData() ?? "{}")).toMatchObject({ scope: { kind: "agent", id: agent.agent_id } });
  write({ workspace: WORKSPACE, agent_id: agent.agent_id, command_id: body.command_id ?? "" });
});
