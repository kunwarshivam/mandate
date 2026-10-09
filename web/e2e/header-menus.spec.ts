import { expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS } from "../src/fixtures/workspace";

/**
 * The header folds alerts and the account menu into "Alerts and account" and the workspace switcher
 * into an icon as it narrows (`e2e/stop-visible.spec.ts`); below 64rem they are in the tab bar's More
 * sheet (DEC-207). Each still opens, shows its items, and throws nothing, at a width where it is shown.
 */

const CASES = [
  { width: 768, button: "More", items: ["Alpaca paper", "Paper workspace", "Alerts", "All settings"], role: "dialog" as const },
  { width: 1024, button: "Alerts and account", items: ["Alerts", "You, owner", "Profile", "Notifications"] },
  { width: 1280, button: "Your account", items: ["You, owner", "Profile", "Notifications"] },
  { width: 1024, button: "Workspace: Paper workspace", items: ["Workspaces (fixture)", "Paper workspace", "Second workspace"] },
  { width: 1280, button: "Workspace: Paper workspace", items: ["Workspaces (fixture)", "Paper workspace", "Second workspace"] },
];

for (const { width, button, items, role = "menu" } of CASES) {
  test(`${width} px: the ${button} menu opens`, async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.setViewportSize({ width, height: 800 });
    await page.goto(recordHref("kill", AGENT_IDS.btc));
    await page.waitForLoadState("networkidle");
    const menu = role === "dialog" ? page.getByRole("dialog", { name: button }) : page.getByRole("menu");
    await expect(async () => {
      const bar = role === "dialog" ? page.getByRole("navigation", { name: "Main" }) : page.getByRole("banner");
      await bar.getByRole("button", { name: button, exact: true }).click();
      await expect(menu).toBeVisible({ timeout: 1000 });
    }).toPass({ timeout: 15_000 });
    for (const item of items) await expect(menu).toContainText(item);
    expect(errors).toEqual([]);
  });
}
