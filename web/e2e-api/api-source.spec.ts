import { type Page, type Request, expect, test } from "@playwright/test";
import { operationFor } from "../src/api/operations";

/**
 * The app with the workspace API as its data source (DEC-975): every screen reads through documented
 * operations only (E21-45), nothing is labelled as fixtures, and an answer is recorded through the
 * API's idempotent write route. The scenario cookie picks what the e2e build's mock API serves.
 */
const WS = process.env.OWLHEAD_API_E2E_WORKSPACE ?? "ws_01J9ZQ4M0000000000000000AB";

function apiCalls(page: Page): Request[] {
  const calls: Request[] = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname.startsWith("/v1/")) calls.push(request);
  });
  return calls;
}

test("Home reads the workspace through documented operations and labels nothing as fixture data", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/?scenario=approvals");
  await expect(page.getByRole("heading", { name: /^Needs you/ })).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Agents" }).getByRole("link")).not.toHaveCount(0);
  expect(calls.length).toBeGreaterThan(3);
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});

test("Skip on a request goes to the approval's responses route once, and shows only what the API recorded", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/?scenario=approvals");
  await page.getByRole("region", { name: /^Needs you/ }).getByRole("link").first().click();
  await page.getByRole("button", { name: "Skip", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Recorded in the journal: you skipped" })).toBeVisible();
  await expect.poll(() => calls.filter((c) => c.method() === "POST").map((c) => new URL(c.url()).pathname)).toEqual([expect.stringMatching(new RegExp(`^/v1/workspaces/${WS}/approvals/[^/]+/responses$`))]);
  const [post] = calls.filter((c) => c.method() === "POST");
  expect(post.headers()["idempotency-key"]).toMatch(/^[A-Za-z0-9_-]{16,64}$/);
  expect(post.headers()["x-mandate-request"]).toBe("1");
  expect(post.postDataJSON()).toMatchObject({ verdict: "skipped", step_up: null, delegation: null });
});

test("Approve shows a code to confirm on the host, and the approve that follows carries that challenge's evidence (DEC-2104)", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/?scenario=approvals");
  await page.getByRole("region", { name: /^Needs you/ }).getByRole("link").first().click();
  await page.getByRole("button", { name: "Approve", exact: true }).click();
  const panel = page.getByRole("region", { name: "Confirm on the host" });
  await expect(panel).toBeVisible();
  const code = (await panel.locator("[data-slot=host-confirm-code]").textContent()) ?? "";
  expect(code).toMatch(/^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/);
  await expect(panel.locator("[data-slot=host-confirm-command]")).toContainText(`mandate step-up confirm --workspace ${WS} --principal `);
  await expect(panel.locator("[data-slot=host-confirm-command]")).toContainText(`--code ${code}`);
  const posts = () => calls.filter((c) => c.method() === "POST");
  expect(posts().map((c) => new URL(c.url()).pathname)).toEqual([`/v1/workspaces/${WS}/step-up/challenges`]);
  const challenge = (await (await posts()[0].response())?.json()) as { challenge_id: string; issued_at: string };
  await panel.getByRole("button", { name: "Approve", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Recorded in the journal: you approved" })).toBeVisible();
  const answer = posts().find((c) => new URL(c.url()).pathname.endsWith("/responses"));
  expect(answer?.postDataJSON()).toMatchObject({ verdict: "approved", step_up: { assertion_id: challenge.challenge_id, authenticated_at: challenge.issued_at, method: "cli_confirm" } });
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});

test("a grant the runtime refused not_an_approver reads as refused in words, never as approved, and is not sent again (DEC-2818)", async ({ page }) => {
  const calls = apiCalls(page);
  await page.route(`**/v1/workspaces/${WS}/commands/*`, async (route) => {
    const served = await route.fetch();
    const body = (await served.json()) as Record<string, unknown>;
    const step = { stream_id: "agent", seq: 9, event_type: "ApprovalResponded", recorded_at: "2026-10-26T15:00:00.000000000Z", reason: "not_an_approver" };
    await route.fulfill({ response: served, json: { ...body, phase: "refused", steps: [step] } });
  });
  await page.goto("/?scenario=approvals");
  await page.getByRole("region", { name: /^Needs you/ }).getByRole("link").first().click();
  await page.getByRole("button", { name: "Approve", exact: true }).click();
  await page.getByRole("region", { name: "Confirm on the host" }).getByRole("button", { name: "Approve", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "Refused: you are not an approver of this request now. Nothing was approved." })).toBeVisible();
  await expect(page.getByText("Recorded in the journal: you approved")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Approve", exact: true })).toHaveCount(0);
  const answers = () => calls.filter((c) => c.method() === "POST" && new URL(c.url()).pathname.endsWith("/responses"));
  await page.waitForTimeout(500);
  expect(answers()).toHaveLength(1);
});

test("Connect Alpaca paper shows the disclosure and a host code, starts with that challenge's evidence, and lands on the connection (DEC-2623, DEC-2626)", async ({ page }) => {
  const calls = apiCalls(page);
  let authorize = "";
  await page.route("https://paper-oauth.mock.invalid/**", async (route) => {
    authorize = route.request().url();
    await route.fulfill({ status: 200, contentType: "text/html", body: "<title>Alpaca (mock)</title>" });
  });
  await page.goto("/connections");
  await expect(page.getByRole("list", { name: "Broker accounts" })).toContainText("Connected.");
  await page.getByRole("button", { name: "Connect Alpaca paper" }).click();
  await expect(page.getByRole("region", { name: "Before you connect Alpaca paper trading" })).toHaveAttribute("data-version", "1");
  await page.getByRole("button", { name: "Connect with Alpaca" }).click();
  const panel = page.getByRole("region", { name: "Confirm on the host" });
  await expect(panel.locator("[data-slot=host-confirm-command]")).toContainText(`mandate step-up confirm-connect --workspace ${WS} --principal `);
  let started = { connection_id: "" };
  await page.route(`**/v1/workspaces/${WS}/connections/oauth/start`, async (route) => {
    const response = await route.fetch();
    started = (await response.json()) as typeof started;
    await route.fulfill({ response });
  });
  await panel.getByRole("button", { name: "Connect with Alpaca" }).click();
  await expect(page).toHaveTitle("Alpaca (mock)");
  expect(started.connection_id).toMatch(/^conn_[0-9A-HJKMNP-TV-Z]{26}$/);
  expect(new URL(authorize).origin).toBe("https://paper-oauth.mock.invalid");
  const start = calls.find((c) => c.method() === "POST" && new URL(c.url()).pathname.endsWith("/connections/oauth/start"));
  expect(start?.postDataJSON()).toMatchObject({ broker: "alpaca", environment: "paper", disclosure: { document: "alpaca_oauth_token_breadth", version: "1" }, step_up: { method: "cli_confirm" } });
  await page.goto(`/connections/${started.connection_id}`);
  await expect(page.getByRole("status").filter({ hasText: /^(Connecting|Connected)/ })).toBeVisible();
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});

test("a deployment that does not answer shows unreachable, not the fixtures", async ({ page }) => {
  await page.goto("/?scenario=unreachable");
  await expect(page.getByRole("heading", { name: "Cannot reach your deployment" })).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
});

test("setting up an agent is served over the API: the conversation, read without a model, with no fixture tag", async ({ page }) => {
  await page.goto("/agents/new?scenario=normal");
  await expect(page.getByRole("textbox", { name: "Your answer" })).toBeVisible();
  await expect(page.getByText("Your deployment doesn't serve a model to read your words yet")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Not available from your deployment yet" })).toHaveCount(0);
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
});

test("positions are read from the API: each agent's own, with what is not served said so, and no fixture tag", async ({ page }) => {
  await page.goto("/positions?scenario=normal");
  await expect(page.getByRole("table", { name: "The account's holdings" })).toBeVisible();
  await expect(page.getByRole("cell", { name: "Not reported" }).first()).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
});

test("the audit timeline reads the journal's streams from the API, newest first, with no fixture tag", async ({ page }) => {
  await page.goto("/audit/timeline?scenario=normal");
  await expect(page.getByRole("list", { name: "Journal records" }).getByRole("listitem").first()).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
});

test("a screen the API does not serve yet says so, and shows no fixture figures", async ({ page }) => {
  await page.goto("/audit/decisions?scenario=approvals");
  await expect(page.getByRole("heading", { name: "Not available from your deployment yet" })).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
});

test("a backtest report reads GET /backtests/{id} and shows the served report, with no fixture tag", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/backtests/bt_01JBR2C4D6E8F0G2H4J6K8M0N2?scenario=normal");
  await expect(page.getByRole("table", { name: "This version and buy and hold" })).toBeVisible();
  await expect(page.getByText("[[LEGEND-HYPOTHETICAL]]")).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
  expect(calls.map((c) => new URL(c.url()).pathname)).toContain(`/v1/workspaces/${WS}/backtests/bt_01JBR2C4D6E8F0G2H4J6K8M0N2`);
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});
