import { writeFileSync } from "node:fs";
import { type Page, type Response, expect, test } from "@playwright/test";
import { virtualPasskey } from "./virtual-passkey";

/**
 * The owner's set-up from web/, against a running local cell on paper (the demo's step 2; lane WB):
 * the owner, signed in through the stub issuer,
 *
 * 1. adds a passkey for confirming in Settings, made by Chromium's virtual authenticator through the
 *    page's own WebAuthn call and the API's real enrolment (`POST /passkeys`), and sees it listed
 *    with its first 24 hours still to run (workspace API spec §3.6, DEC-2108 item 5);
 * 2. sets an agent up in `/agents/new` over the API, answering the conversation in figures, on an
 *    instrument and the model the deployment registered, and creates version 1
 *    (`POST /mandate-versions`, `MandateVersionCreated`);
 * 3. asks to confirm it, and is told in words that the passkey is new and confirms only after its
 *    first 24 hours, with nothing sent: a first passkey's cool-off is a safety rule, and shortening
 *    it, even in a cell, is the founder's decision (DEC-79), so the cell's confirm and deploy stay on
 *    the CLI until then;
 * 4. is offered no deployment, since version 1 is not confirmed.
 *
 * Nothing it records conflicts with the scenario's agent: a version that is never deployed starts
 * no agent. The driver (`cargo xtask cell demo`, step 2) names the workspace and reads the steps this
 * writes to `OWLHEAD_CELL_RESULT`, so a failure names the first step that did not happen. Paper only:
 * no step here can reach a live venue.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const WORKSPACE = process.env.OWLHEAD_WORKSPACE_ID ?? "";
/** The whole test's bound, and one page's: `next dev` compiles each route on its first visit. */
const SETUP_MS = Number(process.env.OWLHEAD_CELL_SETUP_MS ?? 240_000);
const PAGE_MS = 90_000;
/** The demo's figures: inside the paper account's room, and a loss inside the workspace's ceiling. */
const MONEY = "$1,000";
const GOAL = "Follow the trend on paper";
const LOSS = "$100";
const FAST = "5";
const SLOW = "20";

const reached: Record<string, string> = {};
const write = (extra: Record<string, string> = {}) => writeFileSync(RESULT, JSON.stringify({ ...reached, ...extra }));
const step = (name: string, value = "ok") => {
  reached[name] = value;
  write();
};
/** Records why the set-up stopped, then fails, so the trace of what the page did is kept. */
const refuse = (error: string): never => {
  write({ error });
  throw new Error(error);
};

const api = (path: string) => `/v1/workspaces/${WORKSPACE}${path}`;
const isPost = (r: Response, path: string) => r.request().method() === "POST" && new URL(r.url()).pathname === api(path);

/**
 * Opens `path` until its read of `apiPath` is not the one 401: `web/` opens the API session through
 * the stub issuer on its first load, so the first reads may run before the cookie is set. Every pass
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

test("the owner's set-up from web/: a passkey added in Settings, version 1 created in /agents/new, and its confirm held for the passkey's first 24 hours", async ({ page }) => {
  test.setTimeout(SETUP_MS);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE })) expect(value, `the driver names ${name}`).not.toBe("");
  const authenticator = await virtualPasskey(page);
  const posts: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "POST") posts.push(new URL(r.url()).pathname);
  });

  await test.step("the owner adds a passkey for confirming in Settings, through the API's enrolment", async () => {
    const listed = await signedInRead(page, "/settings/profile", api("/passkeys"));
    if (!listed.ok()) refuse(`the API answered ${listed.status()} for the member's passkeys`);
    const section = page.getByRole("region", { name: "Passkeys for confirming" });
    await expect(section.getByRole("button", { name: "Add a passkey" })).toBeVisible({ timeout: PAGE_MS });
    const before = ((await listed.json()) as { passkeys?: unknown[] }).passkeys ?? [];
    if (before.length > 0) {
      step("passkey_enrolled", "listed");
      return;
    }
    const enrolled = page.waitForResponse((r) => isPost(r, "/passkeys"), { timeout: 30_000 });
    await section.getByRole("button", { name: "Add a passkey" }).click();
    const answer = await enrolled;
    if (!answer.ok()) refuse(`the API answered ${answer.status()} for the enrolment: ${(await answer.text()).slice(0, 400)}`);
    await expect(section.locator("[data-slot=passkey-added]")).toContainText("after its first 24 hours", { timeout: 15_000 });
    if ((await authenticator.credentials()) !== 1) refuse("the API recorded an enrolment the browser's authenticator holds no key for");
    await expect(section.getByRole("list", { name: "Your passkeys on this deployment" }).getByRole("listitem")).toContainText(/Can confirm from/);
    step("passkey_enrolled", ((await answer.json()) as { passkey_id?: string }).passkey_id ?? "");
  });

  await test.step("the owner sets an agent up in /agents/new and creates version 1", async () => {
    const instruments = await signedInRead(page, "/agents/new", api("/instruments"));
    if (!instruments.ok()) refuse(`the API answered ${instruments.status()} for the registered instruments`);
    const symbol = (((await instruments.json()) as { instruments?: Array<{ symbol: string }> }).instruments ?? []).map((i) => i.symbol).sort()[0];
    if (!symbol) return refuse("the deployment registered no instrument, so no agent can be set up");
    const answer = page.getByRole("textbox", { name: "Your answer" });
    await expect(answer).toBeVisible({ timeout: PAGE_MS });
    const say = async (text: string) => {
      await answer.fill(text);
      await page.getByRole("button", { name: "Send" }).click();
    };
    for (const text of [MONEY, GOAL, LOSS, symbol]) await say(text);
    const models = page.getByRole("group", { name: "Models" });
    await expect(models).toBeVisible({ timeout: 15_000 });
    await models.getByRole("button").filter({ hasText: "Moving-average crossover" }).click();
    await say(FAST);
    await say(SLOW);
    const created = page.waitForResponse((r) => isPost(r, "/mandate-versions"), { timeout: 30_000 });
    await page.getByRole("button", { name: "Create the version" }).click();
    const reply = await created;
    if (!reply.ok()) refuse(`the API answered ${reply.status()} for the version: ${(await reply.text()).slice(0, 400)}`);
    const version = page.locator("[data-step=version] [data-slot=created-version]");
    await expect(version).toBeVisible({ timeout: 30_000 });
    step("version_created", ((await version.textContent()) ?? "").trim());
  });

  await test.step("the confirm is held in words while the passkey is new, with nothing sent", async () => {
    const before = posts.length;
    await page.getByRole("button", { name: "Confirm version 1" }).click();
    const held = page.locator("[data-step=confirm] [data-reason=passkey_cooling_off]");
    await expect(held).toBeVisible({ timeout: 30_000 });
    await expect(held).toContainText("your passkey is new, and confirms only once its first 24 hours have passed");
    const sent = posts.slice(before).filter((p) => p.endsWith("/confirm") || p === api("/step-up/challenges"));
    if (sent.length > 0) refuse(`the browser sent ${sent.join(", ")} for a confirm its passkey could not make`);
    step("confirm_refused_cooling_off");
  });

  await test.step("no deployment is offered for a version not confirmed", async () => {
    await expect(page.locator("[data-step=deploy]")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Deploy to paper" })).toHaveCount(0);
    if (posts.some((p) => p === api("/agents"))) refuse("the browser sent a deployment for a version it never confirmed");
    step("deploy_not_offered");
  });
});
