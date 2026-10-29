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
 * With `OWLHEAD_CELL_SETUP_DEPLOY=1`, which the driver sets only in a dev or ci cell it started
 * waiving a first passkey's cool-off (DEC-2146), steps 3 and 4 are instead the owner confirming version 1 with the passkey
 * (`POST /mandate-versions/{v}/confirm`) and deploying it to paper (`POST /agents`): web/'s set-up
 * then deploys the run's agent, and the API's id for it is the result's `agent_deployed`, which the
 * driver hands every later step.
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
/**
 * Set by the driver when web/'s set-up deploys the run's agent, which it does only with the cell
 * started waiving a first passkey's cool-off (DEC-2146; the scenario's tests pin the two together).
 * The owner then confirms and deploys version 1 here, and the driver takes the agent's id from
 * `agent_deployed`. Nothing a page can read shows the waiver, so the spec does not check it: in a
 * cell that does not waive, the API answers the confirm `step_up_missing` and the step refuses with
 * that answer. Unset, the confirm is held as the cool-off holds it.
 */
const DEPLOY = process.env.OWLHEAD_CELL_SETUP_DEPLOY === "1";
/**
 * Where the deploying set-up hands its virtual passkey on, so a later run of this cell (the trade
 * loop's resume) can step up with it: a file the driver names under `target/cell` in a dev or ci
 * cell only, deletes at teardown, and the leak guard refuses as an artifact (DEC-2411). Unset, the
 * key stays in this run's browser.
 */
const PASSKEY_FILE = process.env.OWLHEAD_CELL_PASSKEY_FILE ?? "";
/** An agent id as the API mints it, the form the driver accepts. */
const AGENT_ID = /^agt_[A-Za-z0-9]{26}$/;
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
 * Presses a set-up step's button, then the step-up dialog's "Use passkey" when it opens: the
 * virtual authenticator answers the page's own WebAuthn call, so the owner's gesture is the click.
 */
async function passkeyAction(page: Page, name: string) {
  await page.getByRole("button", { name, exact: true }).click({ timeout: 30_000 });
  const dialog = page.getByRole("dialog", { name: "Confirm it is you" });
  const opened = await dialog.waitFor({ state: "visible", timeout: 5_000 }).then(() => true, () => false);
  if (opened) await dialog.getByRole("button", { name: "Use passkey" }).click();
}

/** What a set-up step's item says, for a refusal; bounded, since the cell config sets no action timeout. */
async function stepSaid(page: Page, which: string): Promise<string> {
  return ((await page.locator(`[data-step=${which}]`).textContent({ timeout: 2_000 }).catch(() => null)) ?? "nothing").replace(/\s+/g, " ").slice(0, 300);
}

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

  if (DEPLOY) {
    await test.step("the owner confirms version 1 with the passkey, and the journal records it", async () => {
      const confirmed = page.waitForResponse((r) => r.request().method() === "POST" && /\/mandate-versions\/[^/]+\/confirm$/.test(new URL(r.url()).pathname), { timeout: 60_000 });
      await passkeyAction(page, "Confirm version 1");
      const answer = await confirmed.catch(() => null);
      if (!answer) return refuse(`the browser sent no confirm within 60 s; the confirm step said: ${await stepSaid(page, "confirm")}`);
      if (!answer.ok()) refuse(`the API answered ${answer.status()} for the confirm: ${(await answer.text()).slice(0, 400)}`);
      await expect(page.locator("[data-step=confirm][data-phase=recorded]")).toBeVisible({ timeout: 30_000 });
      step("version_confirmed");
    });

    await test.step("the owner deploys version 1 to paper with the passkey, and the API names the agent", async () => {
      const deployed = page.waitForResponse((r) => isPost(r, "/agents"), { timeout: 60_000 });
      await passkeyAction(page, "Deploy to paper");
      const answer = await deployed.catch(() => null);
      if (!answer) return refuse(`the browser sent no deployment within 60 s; the deploy step said: ${await stepSaid(page, "deploy")}`);
      if (!answer.ok()) refuse(`the API answered ${answer.status()} for the deployment: ${(await answer.text()).slice(0, 400)}`);
      const shown = page.locator("[data-step=deploy] [data-slot=deployed-agent]");
      await expect(shown).toBeVisible({ timeout: 30_000 });
      const agent = ((await shown.textContent()) ?? "").trim();
      if (!AGENT_ID.test(agent)) refuse(`the deployment showed agent "${agent}", not an id of the form agt_ and 26 letters or digits`);
      if (PASSKEY_FILE && (await authenticator.exportTo(PASSKEY_FILE)) < 1) refuse("the deployment was recorded but the browser's authenticator held no passkey to hand on");
      step("agent_deployed", agent);
    });
  } else {
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
  }
});
