import { expect, test } from "@playwright/test";

/**
 * The public pages sit in their own frame (DEC-211): the wordmark home and "Sign in", on the app's
 * tokens and glass header, and none of the app's controls. Sign-in is off in the e2e build, so the
 * pages render without Supabase and the sign-in page says it is off.
 */

const APP_CONTROLS = ["[data-slot=stop-control]", "[data-slot=dock]", "[data-slot=wire]", "[data-slot=environment-badge]"];

for (const width of [390, 1440]) {
  test.describe(`at ${width}px`, () => {
    test.use({ viewport: { width, height: 844 } });

    for (const path of ["/welcome", "/login", "/auth/passkey"]) {
      test(`${path} is in the site frame, with no Stop, dock, wire or paper badge`, async ({ page }) => {
        await page.goto(path);
        const header = page.getByRole("banner");
        await expect(header).toHaveClass(/\bglass\b/);
        await expect(header.getByRole("link", { name: "Owlhead" })).toHaveAttribute("href", "/");
        await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
        for (const selector of APP_CONTROLS) await expect(page.locator(selector), selector).toHaveCount(0);
        await expect(page.getByRole("button", { name: "Stop" })).toHaveCount(0);
        await expect(page.getByRole("navigation", { name: "Main" })).toHaveCount(0);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      });
    }

    test("the welcome page leads to sign in, from the header and from the page", async ({ page }) => {
      await page.goto("/welcome");
      await expect(page.getByRole("banner").getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login");
      await expect(page.getByRole("main").getByRole("link", { name: "Get started" })).toHaveAttribute("href", "/login");
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
