import { expect, test } from "@playwright/test";
import { SITE_ORIGIN } from "../playwright.auth.config";

/**
 * One app origin (DEC-2405): this build serves the public site at `localhost` and the app at
 * `127.0.0.1`, as production serves `owlhead.ai` and `app.owlhead.ai` from one Worker.
 */
test("sign-in and app screens asked of the site move to the app with a 308, query kept", async ({ request, baseURL }) => {
  for (const path of ["/login?next=/agents", "/agents", "/auth/callback?code=x&next=/approvals"]) {
    const response = await request.get(`${SITE_ORIGIN}${path}`, { maxRedirects: 0 });
    expect(response.status(), path).toBe(308);
    expect(response.headers()["location"], path).toBe(`${baseURL}${path}`);
  }
});

test("the site keeps the public pages, and its sign-in link points at the app", async ({ page, baseURL }) => {
  await page.goto(`${SITE_ORIGIN}/`);
  expect(new URL(page.url()).origin).toBe(SITE_ORIGIN);
  await expect(page.locator("[data-slot=landing]")).toBeVisible();
  const links = await page.locator('a[href$="/login"]').evaluateAll((as) => as.map((a) => a.getAttribute("href")));
  expect(links.length).toBeGreaterThan(0);
  for (const href of links) expect(href).toBe(`${baseURL}/login`);
});

test("following sign-in from the site lands on the app's sign-in page", async ({ page, baseURL }) => {
  await page.goto(`${SITE_ORIGIN}/login?next=/agents`);
  expect(page.url()).toBe(`${baseURL}/login?next=/agents`);
  await expect(page.getByRole("button", { name: /Google/ })).toBeVisible();
});

test("www goes to the site", async ({ page }) => {
  const port = new URL(SITE_ORIGIN).port;
  await page.goto(`http://www.localhost:${port}/welcome`);
  expect(page.url()).toBe(`${SITE_ORIGIN}/welcome`);
});
