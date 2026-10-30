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

/** Opens the agent that needs you from Home, then its approval card's request. */
async function openRequest(page: Page) {
  await page.goto("/?scenario=approvals");
  await page.getByRole("list", { name: "Agents" }).getByRole("link").filter({ has: page.locator("[data-slot=needs-you]") }).first().click();
  await page.locator("[data-slot=chat-card][data-kind=approval]").first().getByRole("link", { name: "Review and answer" }).click();
}

test("Home reads the workspace through documented operations and labels nothing as fixture data", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/?scenario=approvals");
  await expect(page.getByRole("heading", { name: "Your agents" })).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Agents" }).getByRole("link")).not.toHaveCount(0);
  await expect(page.locator("[data-slot=needs-you]").first()).toBeVisible();
  expect(calls.length).toBeGreaterThan(1);
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});

test("a message to an agent goes to its messages route once, with an idempotency key, and the thread shows what the API read back (DEC-2293)", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/?scenario=approvals");
  await page.getByRole("list", { name: "Agents" }).getByRole("link").first().click();
  const composer = page.getByRole("textbox").last();
  await composer.fill("How are you doing today?");
  await composer.press("Enter");
  await expect(page.locator("[data-slot=owner-message]").filter({ hasText: "How are you doing today?" })).toBeVisible();
  const posts = () => calls.filter((c) => c.method() === "POST");
  await expect.poll(() => posts().map((c) => new URL(c.url()).pathname)).toEqual([expect.stringMatching(new RegExp(`^/v1/workspaces/${WS}/agents/[^/]+/messages$`))]);
  expect(posts()[0].headers()["idempotency-key"]).toMatch(/^[A-Za-z0-9_-]{16,64}$/);
  expect(posts()[0].headers()["x-mandate-request"]).toBe("1");
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});

test("Skip on a request goes to the approval's responses route once, and shows only what the API recorded", async ({ page }) => {
  const calls = apiCalls(page);
  await openRequest(page);
  await page.getByRole("button", { name: "Skip", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Recorded in the journal: you skipped" })).toBeVisible();
  const answers = () => calls.filter((c) => c.method() === "POST" && new URL(c.url()).pathname.includes("/approvals/"));
  await expect.poll(() => answers().map((c) => new URL(c.url()).pathname)).toEqual([expect.stringMatching(new RegExp(`^/v1/workspaces/${WS}/approvals/[^/]+/responses$`))]);
  const [post] = answers();
  expect(post.headers()["idempotency-key"]).toMatch(/^[A-Za-z0-9_-]{16,64}$/);
  expect(post.headers()["x-mandate-request"]).toBe("1");
  expect(post.postDataJSON()).toMatchObject({ verdict: "skipped", step_up: null, delegation: null });
});

test("Approve shows a code to confirm on the host, and the approve that follows carries that challenge's evidence (DEC-2104)", async ({ page }) => {
  const calls = apiCalls(page);
  await openRequest(page);
  await page.getByRole("button", { name: "Approve", exact: true }).click();
  const panel = page.getByRole("region", { name: "Confirm on the host" });
  await expect(panel).toBeVisible();
  const code = (await panel.locator("[data-slot=host-confirm-code]").textContent()) ?? "";
  expect(code).toMatch(/^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/);
  await expect(panel.locator("[data-slot=host-confirm-command]")).toContainText(`mandate step-up confirm --workspace ${WS} --principal `);
  await expect(panel.locator("[data-slot=host-confirm-command]")).toContainText(`--code ${code}`);
  const posts = () => calls.filter((c) => c.method() === "POST");
  await expect.poll(() => posts().map((c) => new URL(c.url()).pathname)).toEqual([`/v1/workspaces/${WS}/step-up/challenges`]);
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
  await openRequest(page);
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
  await page.goto("/settings");
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

test("setting up an agent is a chat served over the API: the first message opens a setup, with no fixture tag (DEC-2293)", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/new?scenario=normal");
  await expect(page.getByRole("heading", { name: "New agent" })).toBeVisible();
  const composer = page.getByRole("textbox").last();
  await composer.fill("Buy and hold a few large US companies.");
  await composer.press("Enter");
  await expect(page.locator("[data-slot=owner-message]").filter({ hasText: "Buy and hold a few large US companies." })).toBeVisible();
  await expect.poll(() => calls.filter((c) => c.method() === "POST").map((c) => new URL(c.url()).pathname)).toEqual([`/v1/workspaces/${WS}/setup/messages`]);
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});

test("an agent's Details read its positions, goals, mandate and orders from the API, with no fixture tag", async ({ page }) => {
  const calls = apiCalls(page);
  await page.goto("/?scenario=normal");
  await page.getByRole("list", { name: "Agents" }).getByRole("link").first().click();
  await page.getByRole("button", { name: "Details" }).click();
  const drawer = page.locator("[data-slot=details-drawer]");
  for (const name of ["Positions", "Goals", "Mandate", "Activity"]) await expect(drawer.getByRole("region", { name })).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
  for (const call of calls) expect(operationFor(call.method(), call.url(), WS), `${call.method()} ${call.url()}`).not.toBeNull();
});
