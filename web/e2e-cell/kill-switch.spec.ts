import { writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

/**
 * The cell demo's step 6 from web/ (lane W2): the owner, signed in through the stub issuer, opens
 * Stop and presses one agent's kill switch, and the API records it. It writes the workspace and the
 * kill switch's command id to `OWLHEAD_CELL_RESULT`, which the scenario checks against the
 * workspace's control stream; with no agent in the workspace it writes the reason and presses
 * nothing, never falling back to a workspace-wide switch the owner did not ask for.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const WORKSPACE = process.env.OWLHEAD_WORKSPACE_ID ?? "";

const write = (body: Record<string, string>) => writeFileSync(RESULT, JSON.stringify(body));

test("the kill switch from web/, recorded through the API", async ({ page }) => {
  expect(RESULT, "the driver names where the result goes").not.toBe("");
  const agents = page.waitForResponse((r) => r.request().method() === "GET" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/agents` && r.ok());
  await page.goto("/agents");
  const listed = (await (await agents).json()) as { agents?: Array<{ agent_id: string; mode: string }> };
  const agent = listed.agents?.find((a) => a.mode !== "stopped");
  if (!agent) {
    write({ error: "no agent in the API's workspace to kill" });
    return;
  }
  const detail = page.waitForResponse((r) => r.request().method() === "GET" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/agents/${agent.agent_id}` && r.ok());
  await page.goto(`/agents/${agent.agent_id}`);
  await detail;
  await expect(page.getByRole("main")).not.toBeEmpty();
  await page.getByRole("button", { name: "Stop", exact: true }).first().click();
  const sheet = page.getByRole("dialog", { name: /^Stop/ });
  await expect(sheet).toBeVisible();
  await sheet.getByRole("link", { name: /^Kill switch:/ }).first().click();
  await expect(page).toHaveURL(new RegExp(`/agents/${agent.agent_id}/kill-switch$`));
  const recorded = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname === `/v1/workspaces/${WORKSPACE}/kill-switch`);
  await page.getByRole("button", { name: /Activate the kill switch/ }).click();
  const passkey = page.getByRole("dialog", { name: "Confirm it is you" });
  const first = await Promise.race([recorded.then(() => "sent" as const), passkey.waitFor({ state: "visible" }).then(() => "passkey" as const)]);
  if (first === "passkey") await passkey.getByRole("button", { name: "Use passkey" }).click();
  const answer = await recorded;
  expect(answer.status(), "the API recorded the kill switch").toBe(202);
  const body = (await answer.json()) as { command_id?: string; phase?: string };
  expect(body.phase).toBe("recorded");
  expect(JSON.parse(answer.request().postData() ?? "{}")).toMatchObject({ scope: { kind: "agent", id: agent.agent_id } });
  write({ workspace: WORKSPACE, agent_id: agent.agent_id, command_id: body.command_id ?? "" });
});
