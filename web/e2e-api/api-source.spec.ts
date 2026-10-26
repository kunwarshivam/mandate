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

test("a deployment that does not answer shows unreachable, not the fixtures", async ({ page }) => {
  await page.goto("/?scenario=unreachable");
  await expect(page.getByRole("heading", { name: "Cannot reach your deployment" })).toBeVisible();
  await expect(page.locator("[data-slot=fixture-tag]")).toHaveCount(0);
});

test("a screen the API does not serve yet says so, and shows no fixture figures", async ({ page }) => {
  await page.goto("/positions?scenario=approvals");
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
