import { expect, test } from "@playwright/test";

/**
 * The public pages sit in their own frame (DEC-211): the wordmark home and "Sign in", on the app's
 * tokens and glass header, and none of the app's controls. The landing page draws its own window
 * instead of the header (DEC-213). Sign-in is off in the e2e build, so the pages render without
 * Supabase and the sign-in page says it is off.
 */

const APP_CONTROLS = ["[data-slot=stop-control]", "[data-slot=dock]", "[data-slot=wire]"];

for (const width of [390, 1440]) {
  test.describe(`at ${width}px`, () => {
    test.use({ viewport: { width, height: 844 } });

    test("/welcome has none of the app's controls and no site header", async ({ page }) => {
      await page.goto("/welcome");
      await expect(page.getByRole("heading", { level: 1, name: "Owlhead" })).toBeVisible();
      await expect(page.locator("header.glass")).toHaveCount(0);
      for (const selector of APP_CONTROLS) await expect(page.locator(selector), selector).toHaveCount(0);
      await expect(page.getByRole("button", { name: "Stop" })).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    });

    for (const path of ["/login", "/auth/passkey"]) {
      test(`${path} is in the site frame, with no Stop, dock, wire or paper badge`, async ({ page }) => {
        await page.goto(path);
        const header = page.getByRole("banner");
        await expect(header).toHaveClass(/\bglass\b/);
        await expect(header.getByRole("link", { name: "Owlhead" })).toHaveAttribute("href", "/");
        await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
        for (const selector of APP_CONTROLS) await expect(page.locator(selector), selector).toHaveCount(0);
        await expect(header.locator("[data-slot=environment-badge]")).toHaveCount(0);
        await expect(page.getByRole("button", { name: "Stop" })).toHaveCount(0);
        await expect(page.getByRole("navigation", { name: "Main" })).toHaveCount(0);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      });
    }

    test("the welcome page leads to sign in, from its header and its footer", async ({ page }) => {
      await page.goto("/welcome");
      const links = page.getByRole("link", { name: "Sign in" });
      await expect(links).toHaveCount(2);
      for (const link of await links.all()) await expect(link).toHaveAttribute("href", "/login");
    });

    test("the sign-in page has no link to itself, and says sign-in is off in this build", async ({ page }) => {
      await page.goto("/login");
      await expect(page.getByRole("banner").getByRole("link", { name: "Sign in" })).toHaveCount(0);
      await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
      await expect(page.getByText(/Sign-in is off in this build/)).toBeVisible();
    });
  });
}

test("the landing page is the one indexed public page, each with its own title", async ({ page }) => {
  await page.goto("/welcome");
  await expect(page).toHaveTitle(/^Owlhead$/);
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute("content", /^index, follow$/);
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute("href", "https://owlhead.ai");
  for (const [path, title] of [
    ["/login", /^Sign in/],
    ["/auth/passkey", /^Add a passkey/],
  ] as const) {
    await page.goto(path);
    await expect(page).toHaveTitle(title);
    await expect(page.locator('meta[name="robots"]')).toHaveAttribute("content", /noindex/);
  }
});
