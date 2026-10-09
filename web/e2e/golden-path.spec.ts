import { type Page, expect, test } from "@playwright/test";

/**
 * The golden path (web/design/plan.md, section A): sign in, set up an agent, see it ask, approve,
 * read the record, stop it. The ten minutes that must be perfect on every release, walked as an
 * owner walks them, at a phone's width and a desktop's, in both themes (the light and dark
 * projects). One page load after sign-in, so the fixture runtime's state carries from step to step;
 * every move after it is a press, and every check is what the owner sees (DEC-511).
 */

const WIDTHS = [390, 1440] as const;
/** Tailwind's `lg`: from here up the dock carries the nav, below it the tab bar (DEC-207). */
const DESKTOP = 1024;

const REQUEST = /^Agent 4 asks to buy 6 MSFT at a limit of \$44\.62/;

/** The dock on a desktop, the tab bar on a phone. */
const appNav = (page: Page, width: number) => page.getByRole("navigation", { name: width < DESKTOP ? "Main" : "Primary" });
/** An agent's sections: tabs under its title on a desktop, a list at the foot of the page on a phone. */
const agentSections = (page: Page, width: number) => page.getByRole("navigation", { name: width < DESKTOP ? "This agent" : "Agent sections" });

const conversation = (page: Page) => page.getByRole("log", { name: "Conversation" });
const composer = (page: Page) => page.getByRole("textbox", { name: "Your message" });

/** Sends one message in set-up and waits until the model has read it. */
async function say(page: Page, text: string) {
  await composer(page).fill(text);
  await composer(page).press("Enter");
  await expect(conversation(page).locator("[data-slot=owner-message]").last()).toHaveText(`You: ${text}`);
  await expect(page.locator("[data-slot=thinking]")).toHaveText("");
}

/** This build runs on fixtures with sign-in off: the sign-in page opens the workspace without an account. */
async function signIn(page: Page, width: number) {
  await page.setViewportSize({ width, height: width < DESKTOP ? 844 : 900 });
  await page.goto("/login");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.getByRole("link", { name: "Open Owlhead" }).click();
  await expect(page).toHaveURL("/");
  await expect(page.getByRole("heading", { name: /^Needs you/ })).toBeVisible();
}

/** From Agents, the set-up conversation, then Create agent with a passkey, until its first check asks. */
async function setUpAgent(page: Page, width: number) {
  await appNav(page, width).getByRole("link", { name: "Agents" }).click();
  await page.getByRole("link", { name: "Describe an agent" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Set up an agent" })).toBeVisible();
  await say(page, "about three grand");
  await say(page, "Grow it steadily");
  await say(page, "$300");
  await say(page, "MSFT");
  await page.getByRole("group", { name: "Models" }).getByRole("button", { name: /^Momentum/ }).click();
  await say(page, "20");
  await page.getByRole("button", { name: "Create agent" }).click();
  const stepUp = page.getByRole("dialog", { name: "Confirm it is you" });
  await expect(stepUp).toContainText("Create this agent on paper with $3,000.00 of simulated money, trading MSFT.");
  await stepUp.getByRole("button", { name: "Use passkey" }).click();
  const after = page.locator("[data-slot=after-confirm]");
  await expect(after).toContainText("Agent 4 is running on paper");
  await expect(after).toContainText(/Its first check asks you: buy 6 MSFT at \$44\.62\./);
}

async function goHome(page: Page, width: number) {
  await appNav(page, width).getByRole("link", { name: "Home" }).click();
  await expect(page).toHaveURL("/");
}

for (const width of WIDTHS) {
  test.describe(`${width} px`, () => {
    test("sign in, set up an agent, see it ask, approve, read the record, stop it", async ({ page }) => {
      test.setTimeout(90_000);

      await test.step("1. Sign in", async () => {
        await signIn(page, width);
      });

      await test.step("2. Set up an agent: the conversation, then Create agent with a passkey", async () => {
        await setUpAgent(page, width);
      });

      await test.step("3. See it ask: the request waits under Needs you on Home", async () => {
        await goHome(page, width);
        const request = page.getByRole("region", { name: /^Needs you/ }).getByRole("link", { name: REQUEST });
        await expect(request).toHaveCount(1);
        await request.click();
        await expect(page).toHaveURL(/\/approvals\/apr_[0-9A-Z]{26}$/);
        await expect(page.getByRole("main")).toContainText("Your mandate asks before every buy.");
      });

      await test.step("4. Approve: a paper buy inside its limits takes no step-up; it ends filled", async () => {
        await page.getByRole("button", { name: "Approve", exact: true }).click();
        await expect(page.getByRole("heading", { name: "You chose Approve" })).toBeVisible();
        const status = page.getByRole("status").filter({ hasText: "Approved by you." });
        await expect(status).toContainText("submitted and filled 6 at $44.62", { timeout: 15_000 });
      });

      await test.step("5. Read the record: the decision, attributed and on paper; the agent's P&L carries its disclosure", async () => {
        await appNav(page, width).getByRole("link", { name: "Agents" }).click();
        await expect(page).toHaveURL("/agents");
        await page.getByRole("main").getByRole("link", { name: /^Agent 4/ }).first().click();
        await expect(page).toHaveURL(/\/agents\/agt_[0-9A-Z]{26}$/);
        const main = page.getByRole("main");
        const disclosure = main.getByRole("button", { name: "Performance disclosure" }).filter({ visible: true }).first();
        await expect(disclosure).toBeVisible();
        await expect(disclosure).toHaveAccessibleDescription("[[DISCLOSURE-PERFORMANCE]]");

        const decisions = agentSections(page, width).getByRole("link", { name: "Decisions" });
        await decisions.click();
        await expect(decisions).toHaveAttribute("aria-current", "page");
        await main.getByRole("listitem").filter({ hasText: "Approved by you; submitted and filled." }).getByRole("link").click();
        await expect(page).toHaveURL(/\/decisions\/[0-9A-Z]{26}$/);
        await expect(page.getByRole("heading", { level: 1, name: "Gate decision" })).toBeVisible();
        await expect(main).toContainText("Buy 6 MSFT at $44.62, Agent 4");
        await expect(main.getByRole("region", { name: "Verdict" })).toContainText("Approved by you; submitted and filled.");
        await expect(page.getByText("PAPER", { exact: true }).filter({ visible: true }).first()).toBeVisible();
        await expect(main.getByRole("link", { name: "The request it sent you" })).toBeVisible();
      });

      await test.step("6. Stop it: Stop opens, the kill switch confirms with a passkey, and the agent is stopped", async () => {
        await page.getByRole("button", { name: "Stop", exact: true }).click();
        const sheet = page.getByRole("dialog", { name: /^Stop/ });
        await expect(sheet).toBeVisible();
        await sheet.getByRole("link", { name: /Kill switch: close and stop/ }).click();
        await expect(page).toHaveURL(/\/agents\/agt_[0-9A-Z]{26}\/kill-switch$/);
        await page.getByRole("button", { name: /Activate the kill switch/ }).click();
        await page.getByRole("dialog", { name: "Confirm it is you" }).getByRole("button", { name: "Use passkey" }).click();
        await expect(page.locator("[data-slot=after-confirm]")).toContainText("Stopped");

        await appNav(page, width).getByRole("link", { name: "Agents" }).click();
        await expect(page).toHaveURL("/agents");
        await page.getByRole("main").getByRole("link", { name: /^Agent 4/ }).first().click();
        await expect(page).toHaveURL(/\/agents\/agt_[0-9A-Z]{26}$/);
        await expect(page.getByRole("main").getByText("Stopped", { exact: true }).filter({ visible: true }).first()).toBeVisible();
      });
    });

    test('3. See it ask, once: the open request appears once on Home (C-5)', async ({ page }) => {
      await signIn(page, width);
      await setUpAgent(page, width);
      await goHome(page, width);
      await expect(page.getByRole("main").getByText(/buy 6 MSFT at/i).filter({ visible: true })).toHaveCount(1);
    });
  });
}
