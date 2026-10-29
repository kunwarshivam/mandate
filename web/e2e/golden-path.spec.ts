import { type Page, expect, test } from "@playwright/test";

/**
 * The golden path of the chat-first app (DEC-2293): sign in, see your agents, open the one that
 * needs you, answer the approval it asked in its thread, talk to it, read its Details, and stop it
 * with its kill switch. At a phone's width and a desktop's, in both themes. One page load after
 * sign-in, so the fixture runtime's state carries from step to step; every move after it is a
 * press, and every check is what the owner sees.
 */

const WIDTHS = [390, 1440] as const;
/** Tailwind's `lg`: from here up the dock carries the nav, below it the tab bar (DEC-207). */
const DESKTOP = 1024;

const appNav = (page: Page, width: number) => page.getByRole("navigation", { name: width < DESKTOP ? "Main" : "Primary" });

/** This build runs on fixtures with sign-in off: the sign-in page opens the workspace without an account. */
async function signIn(page: Page, width: number) {
  await page.setViewportSize({ width, height: width < DESKTOP ? 844 : 900 });
  await page.goto("/login?scenario=approvals");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.getByRole("link", { name: "Open Owlhead" }).click();
  await expect(page).toHaveURL("/");
  await expect(page.getByRole("heading", { level: 1, name: "Your agents" })).toBeVisible();
}

for (const width of WIDTHS) {
  test.describe(`${width} px`, () => {
    test("sign in, open the agent that needs you, answer it, talk to it, read Details, stop it", async ({ page }) => {
      test.setTimeout(90_000);

      await test.step("1. Sign in and see your agents as conversations", async () => {
        await signIn(page, width);
        await expect(page.locator("[data-slot=home-agent]").first()).toBeVisible();
      });

      await test.step("2. Open the agent that needs you: its thread asks for your OK as a card", async () => {
        await page.locator("[data-slot=home-agent]").filter({ has: page.locator("[data-slot=needs-you]") }).first().click();
        await expect(page).toHaveURL(/\/agents\/agt_[0-9A-Z]{26}$/);
        const card = page.getByRole("region", { name: "Needs your OK" }).first();
        await expect(card).toBeVisible();
        await card.getByRole("link", { name: "Review and answer" }).click();
        await expect(page).toHaveURL(/\/approvals\/apr_[0-9A-Z]{26}$/);
      });

      await test.step("3. Approve: a paper buy inside its limits takes no step-up; it ends filled", async () => {
        await page.getByRole("button", { name: "Approve", exact: true }).click();
        await expect(page.getByRole("heading", { name: "You chose Approve" })).toBeVisible();
        const status = page.getByRole("status").filter({ hasText: "Approved by you." });
        await expect(status).toContainText("submitted and filled", { timeout: 15_000 });
        await page.getByRole("link", { name: "Your agents" }).click();
        await expect(page).toHaveURL("/");
      });

      await test.step("4. Talk to an agent: the message and its answer come back from the thread", async () => {
        await page.locator("[data-slot=home-agent]").first().click();
        const field = page.getByRole("textbox").last();
        await field.fill("how am I doing?");
        await field.press("Enter");
        await expect(page.locator("[data-slot=owner-message]").last()).toHaveText("You: how am I doing?");
        await expect(page.locator("[data-slot=agent-message]").last()).toContainText("recorded demo data");
      });

      await test.step("5. Read Details: positions, goals, mandate and activity, read only", async () => {
        await page.getByRole("button", { name: "Details" }).click();
        const drawer = page.getByRole("dialog", { name: "Details" });
        for (const name of ["Positions", "Goals", "Mandate", "Activity"]) await expect(drawer.getByRole("region", { name })).toBeVisible();
        await expect(drawer.getByText("Capital")).toBeVisible();
        await drawer.getByRole("button", { name: "Close details" }).click();
      });

      await test.step("6. Stop it: the header opens Stop, and the kill switch confirms", async () => {
        await page.locator("[data-slot=agent-stop-controls]").click();
        const sheet = page.getByRole("dialog", { name: /^Stop/ });
        await expect(sheet).toBeVisible();
        await sheet.getByRole("link", { name: /Kill switch: close and stop/ }).click();
        await expect(page).toHaveURL(/\/agents\/agt_[0-9A-Z]{26}\/kill-switch$/);
        const activate = page.getByRole("button", { name: /Activate the kill switch/ });
        await expect(async () => {
          const refresh = page.getByRole("button", { name: "Show the current version" });
          if (await refresh.isVisible()) await refresh.click();
          await expect(activate).toBeVisible({ timeout: 1000 });
        }, "the record is current, since the buy it approved may have filled meanwhile").toPass({ timeout: 15_000 });
        await activate.click();
        await page.getByRole("dialog", { name: "Confirm the kill switch" }).getByRole("button", { name: "Activate" }).click();
        await expect(page.locator("[data-slot=after-confirm]")).toContainText("Stopped");
        await appNav(page, width).locator(`a[href="/"]`).click();
        await expect(page).toHaveURL("/");
      });
    });
  });
}
