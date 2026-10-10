import { expect, test } from "@playwright/test";

/**
 * The public pages carry none of the app's controls (DEC-211). The landing page draws its own window
 * on a desktop (DEC-213), and the sign-in pages are a logon window on the same wallpaper, in the same
 * faces, whose close box goes home (DEC-469). Sign-in is off in the e2e build, so the pages render
 * without Supabase and the sign-in page says it is off. The landing page's browser shows the app on the
 * example workspace in a frame (DEC-906), a document of its own at `/demo`, so none of its controls
 * are the page's.
 */

const APP_CONTROLS = ["[data-slot=stop-control]", "[data-slot=dock]"];

for (const width of [390, 1440]) {
  test.describe(`at ${width}px`, () => {
    test.use({ viewport: { width, height: 844 } });

    test("/welcome has none of the app's controls and no site header", async ({ page }) => {
      await page.goto("/welcome");
      await page.getByRole("tab", { name: "Owlhead Home Page" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Owlhead" })).toBeVisible();
      await expect(page.locator("header.glass")).toHaveCount(0);
      for (const selector of APP_CONTROLS) await expect(page.locator(selector), selector).toHaveCount(0);
      await expect(page.getByRole("button", { name: "Stop", exact: true })).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    });

    for (const path of ["/login", "/auth/passkey"]) {
      test(`${path} is a logon window on the landing page's wallpaper, with no Stop, dock or paper badge`, async ({ page }) => {
        await page.goto(path);
        await expect(page.locator("header.glass")).toHaveCount(0);
        const logon = page.locator("[data-slot=logon-window]");
        await expect(logon).toBeVisible();
        await expect(page.locator("[data-slot=logon] [data-slot=wallpaper]")).toHaveCount(1);
        await expect(logon.getByRole("link", { name: /back to the Owlhead home page/ })).toHaveAttribute("href", "/");
        await expect(logon.getByRole("heading", { level: 1 })).toBeVisible();
        expect(await logon.getByRole("heading", { level: 1 }).evaluate((h) => getComputedStyle(h).fontFamily)).toMatch(/Pixelify/);
        for (const selector of APP_CONTROLS) await expect(page.locator(selector), selector).toHaveCount(0);
        await expect(page.locator("[data-slot=environment-badge]")).toHaveCount(0);
        await expect(page.getByRole("button", { name: "Stop", exact: true })).toHaveCount(0);
        await expect(page.getByRole("navigation", { name: "Main" })).toHaveCount(0);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      });
    }

    test("the welcome page leads to sign in, from its header and its footer", async ({ page }) => {
      await page.goto("/welcome");
      await page.getByRole("tab", { name: "Owlhead Home Page" }).click();
      // The retro desktop's Start menu carries a third Sign in among its scenery, and the page's
      // own header and footer sit inside the desktop window's region landmark, so they are scoped
      // structurally: the hero's action row and the footer element.
      const header = page.locator("[data-slot=hero-actions]").getByRole("link", { name: "Sign in" });
      const footer = page.locator("footer").getByRole("link", { name: "Sign in" });
      await expect(header).toHaveAttribute("href", "/login");
      await expect(footer).toHaveAttribute("href", "/login");
    });

    test("the sign-in page has no link to itself, and says sign-in is off in this build", async ({ page }) => {
      await page.goto("/login");
      await expect(page.getByRole("link", { name: "Sign in" })).toHaveCount(0);
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
