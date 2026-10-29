import { writeFileSync } from "node:fs";
import { type Page, type Response, expect, test } from "@playwright/test";
import { newAgentRequest } from "../src/test/new-agent";
import { virtualPasskey } from "./virtual-passkey";

/**
 * The owner's set-up from web/, against a running local cell on paper (the demo's step 2; lane WB),
 * on the chat screens (DEC-2293): the owner, signed in through the stub issuer,
 *
 * 1. adds a passkey for confirming in Settings, made by Chromium's virtual authenticator through the
 *    page's own WebAuthn call and the API's real enrolment (`POST /passkeys`), and sees it listed
 *    with its first 24 hours still to run (workspace API spec §3.6, DEC-2108 item 5);
 * 2. has version 1 created over the API with the page's own session (`POST /mandate-versions`,
 *    `MandateVersionCreated`), on an instrument and the model the deployment registered: the local
 *    cell's model gateway serves no chat model, so the spec stands in for the setup thread's draft,
 *    and opens the version at `/new?version={hash}`, where web/ shows the confirm card the thread
 *    would;
 * 3. asks to confirm it, and is refused while the passkey is new: a first passkey's cool-off is a
 *    safety rule, and shortening it, even in a cell, is the founder's decision (DEC-79), so the
 *    cell's confirm and deploy stay on the CLI until then;
 * 4. is offered no deployment, since version 1 is not confirmed.
 *
 * With `OWLHEAD_CELL_SETUP_DEPLOY=1`, which the driver sets only in a dev or ci cell it started
 * waiving a first passkey's cool-off (DEC-2146), steps 3 and 4 are instead the owner confirming version 1 with the passkey
 * (`POST /mandate-versions/{v}/confirm`) and deploying it to paper (`POST /agents`) from the card:
 * web/ then opens the run's agent, and the API's id for it is the result's `agent_deployed`, which
 * the driver hands every later step.
 *
 * Nothing it records conflicts with the scenario's agent: a version that is never deployed starts
 * no agent. The driver (`cargo xtask cell demo`, step 2) names the workspace and reads the steps this
 * writes to `OWLHEAD_CELL_RESULT`, so a failure names the first step that did not happen. Paper only:
 * no step here can reach a live venue.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const WORKSPACE = process.env.OWLHEAD_WORKSPACE_ID ?? "";
const API = process.env.OWLHEAD_API_ORIGIN ?? "";
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
/** An agent id as the API mints it, the form the driver accepts. */
const AGENT_ID = /^agt_[A-Za-z0-9]{26}$/;
/** The demo's figures: inside the paper account's room, and a loss inside the workspace's ceiling. */
const MONEY = "1000";
const LOSS_FRACTION = "0.1";
const GOAL = "Follow the trend on paper";
/** The moving-average crossover's settings, in the order the model lists its parameters. */
const SETTINGS = ["5", "20"];
const CROSSOVER = "quant.ma_crossover";

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
/** The deployment route, `POST /agents` (§4.2), as a pattern over the API's paths. */
const DEPLOYS = /^\/v1\/workspaces\/[^/]+\/agents$/;

/**
 * Presses a confirm card's button, then the step-up dialog's "Use passkey" when it opens: the
 * virtual authenticator answers the page's own WebAuthn call, so the owner's gesture is the click.
 */
async function passkeyAction(page: Page, name: string) {
  await page.getByRole("button", { name, exact: true }).click({ timeout: 30_000 });
  const dialog = page.getByRole("dialog", { name: "Confirm it is you" });
  const opened = await dialog.waitFor({ state: "visible", timeout: 5_000 }).then(() => true, () => false);
  if (opened) await dialog.getByRole("button", { name: "Use passkey" }).click();
}

/** The confirm card on `/new?version=`, as the setup thread shows a draft. */
const card = (page: Page) => page.locator("[data-slot=chat-card][data-kind=confirm_mandate]");

/** What the confirm card's status says, for a refusal; bounded, since the cell config sets no action timeout. */
async function cardSaid(page: Page): Promise<string> {
  return ((await card(page).getByRole("status").textContent({ timeout: 2_000 }).catch(() => null)) ?? "nothing").replace(/\s+/g, " ").slice(0, 300);
}

/**
 * Version 1's mandate document, as the setup thread's draft would make it: the demo's figures on the
 * fixture request's terms, on the deployment's Alpaca paper connection, its first registered
 * instrument and its moving-average crossover, all read through the API with the page's session.
 */
async function demoMandate(page: Page): Promise<Record<string, unknown> | string> {
  const read = await page.evaluate(
    async ({ api, workspace }) => {
      const get = async (path: string) => {
        const response = await fetch(`${api}/v1/workspaces/${workspace}/${path}`, { credentials: "include", headers: { Accept: "application/json" } });
        return response.ok ? ((await response.json()) as Record<string, unknown>) : `${path} answered ${response.status}`;
      };
      return { connections: await get("connections"), instruments: await get("instruments"), models: await get("signal-models") };
    },
    { api: API, workspace: WORKSPACE },
  );
  for (const answer of Object.values(read)) if (typeof answer === "string") return answer;
  const connections = ((read.connections as { connections?: Array<{ connection_id: string; broker: string; environment: string }> }).connections ?? []).filter((c) => c.broker === "alpaca" && c.environment === "paper");
  const instruments = ((read.instruments as { instruments?: Array<{ instrument_id: string; symbol: string; asset_class: string }> }).instruments ?? []).filter((i) => i.asset_class === "us_equity").sort((x, y) => x.symbol.localeCompare(y.symbol));
  const models = (read.models as { models?: Array<{ model_id: string; model_version: string; content_hash: string; params: string[] }> }).models ?? [];
  const model = models.find((m) => m.model_id === CROSSOVER);
  if (connections.length === 0) return "the deployment holds no Alpaca paper connection";
  if (instruments.length === 0) return "the deployment registered no instrument, so no agent can be set up";
  if (!model) return `the deployment registered no ${CROSSOVER}`;
  const [instrument] = instruments;
  const { mandate } = newAgentRequest(connections[0].connection_id);
  return {
    ...mandate,
    name: `${instrument.symbol.toLowerCase().replaceAll(".", "")}-agent`,
    capital: { allocation_usd: MONEY, max_loss_from_allocation: LOSS_FRACTION },
    universe: { ...mandate.universe, pinned_instruments: [{ asset_id: instrument.instrument_id, symbol: instrument.symbol, asset_class: "us_equity" }] },
    behavior: {
      ...mandate.behavior,
      description: GOAL,
      signal_models: [{ ...mandate.behavior.signal_models[0], id: model.model_id, version: model.model_version, content_hash: model.content_hash, params: model.params.map((key, i) => ({ key, value: SETTINGS[i] ?? SETTINGS[0] })) }],
    },
    risk: { ...mandate.risk, max_position_usd: "250", max_gross_exposure_usd: MONEY, max_order_usd: "100" },
  };
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

test("the owner's set-up from web/: a passkey added in Settings, version 1 opened on its confirm card, and its confirm refused for the passkey's first 24 hours", async ({ page }) => {
  test.setTimeout(SETUP_MS);
  for (const [name, value] of Object.entries({ RESULT, WORKSPACE, API })) expect(value, `the driver names ${name}`).not.toBe("");
  const authenticator = await virtualPasskey(page);
  const posts: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "POST") posts.push(new URL(r.url()).pathname);
  });

  await test.step("the owner adds a passkey for confirming in Settings, through the API's enrolment", async () => {
    const listed = await signedInRead(page, "/settings", api("/passkeys"));
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

  await test.step("version 1 is created over the API, and opens on its confirm card in /new", async () => {
    const mandate = await demoMandate(page);
    if (typeof mandate === "string") return refuse(mandate);
    const created = await page.evaluate(
      async ({ api, workspace, body }) => {
        const response = await fetch(`${api}/v1/workspaces/${workspace}/mandate-versions`, {
          method: "POST",
          credentials: "include",
          headers: { Accept: "application/json", "Content-Type": "application/json", "X-Mandate-Request": "1", "Idempotency-Key": `cellsetup${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}` },
          body: JSON.stringify(body),
        });
        return { status: response.status, body: await response.text() };
      },
      { api: API, workspace: WORKSPACE, body: mandate },
    );
    if (created.status !== 200 && created.status !== 202) return refuse(`the API answered ${created.status} for the version: ${created.body.slice(0, 400)}`);
    const version = (JSON.parse(created.body) as { mandate_version?: string }).mandate_version ?? "";
    if (!/^sha256:[0-9a-f]{64}$/.test(version)) return refuse(`the API named the version "${version}"`);
    await page.goto(`/new?version=${encodeURIComponent(version)}`, { timeout: PAGE_MS });
    await expect(card(page)).toContainText(version, { timeout: PAGE_MS });
    await expect(card(page).getByRole("button", { name: "Confirm this agent" })).toBeVisible({ timeout: 30_000 });
    step("version_created", version);
  });

  if (DEPLOY) {
    await test.step("the owner confirms version 1 with the passkey, and the journal records it", async () => {
      const confirmed = page.waitForResponse((r) => r.request().method() === "POST" && /\/mandate-versions\/[^/]+\/confirm$/.test(new URL(r.url()).pathname), { timeout: 60_000 });
      await passkeyAction(page, "Confirm this agent");
      const answer = await confirmed.catch(() => null);
      if (!answer) return refuse(`the browser sent no confirm within 60 s; the card said: ${await cardSaid(page)}`);
      if (!answer.ok()) refuse(`the API answered ${answer.status()} for the confirm: ${(await answer.text()).slice(0, 400)}`);
      await expect(card(page).getByRole("status")).toContainText("Confirmed.", { timeout: 30_000 });
      step("version_confirmed");
    });

    await test.step("the owner deploys version 1 to paper with the passkey, and the API names the agent", async () => {
      const deployed = page.waitForResponse((r) => r.request().method() === "POST" && DEPLOYS.test(new URL(r.url()).pathname), { timeout: 60_000 });
      await passkeyAction(page, "Deploy to paper");
      const answer = await deployed.catch(() => null);
      if (!answer) return refuse(`the browser sent no deployment within 60 s; the card said: ${await cardSaid(page)}`);
      if (!answer.ok()) refuse(`the API answered ${answer.status()} for the deployment: ${(await answer.text()).slice(0, 400)}`);
      await page.waitForURL(/\/agents\/agt_[A-Za-z0-9]{26}$/, { timeout: 30_000 }).catch(() => null);
      const agent = new URL(page.url()).pathname.split("/").at(-1) ?? "";
      if (!AGENT_ID.test(agent)) refuse(`the deployment showed agent "${agent}", not an id of the form agt_ and 26 letters or digits`);
      step("agent_deployed", agent);
    });
  } else {
    await test.step("the confirm is refused while the passkey is new, and nothing is confirmed", async () => {
      const before = posts.length;
      await passkeyAction(page, "Confirm this agent");
      const said = card(page).getByRole("status");
      const refused = await expect(said)
        .toContainText(/Refused|Not sent/, { timeout: 30_000 })
        .then(() => true, () => false);
      if (!refused) return refuse(`a confirm with a new passkey was not refused; the card said: ${await cardSaid(page)}`);
      if (await card(page).getByText("Confirmed.").count()) refuse("the card read confirmed for a passkey in its first 24 hours");
      step("confirm_refused_cooling_off", posts.slice(before).some((p) => p.endsWith("/confirm")) ? "refused_by_api" : "not_sent");
    });

    await test.step("no deployment is offered for a version not confirmed", async () => {
      await expect(page.getByRole("button", { name: "Deploy to paper" })).toHaveCount(0);
      if (posts.some((p) => DEPLOYS.test(p))) refuse("the browser sent a deployment for a version it never confirmed");
      step("deploy_not_offered");
    });
  }
});
