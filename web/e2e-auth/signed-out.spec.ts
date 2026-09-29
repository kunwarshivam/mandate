import { expect, test } from "@playwright/test";
import { API_VERSION, STUB_SUPABASE, stubSupabase } from "./stub";

/**
 * Sign-in on, no one signed in (DEC-211). The proxy's decisions run for real against the build; the
 * browser's calls to Supabase are answered by `stubSupabase`.
 */

test("/ shows the welcome page and keeps the address /", async ({ page, baseURL }) => {
  const response = await page.goto("/");
  expect(response?.status()).toBe(200);
  expect(response?.request().redirectedFrom()).toBeNull();
  await expect(page).toHaveURL(`${baseURL}/`);
  await expect(page.locator("[data-slot=landing]")).toBeVisible();
  await expect(page.getByRole("main").getByRole("link", { name: "Get started" })).toHaveAttribute("href", "/login");
  await expect(page.locator("[data-slot=stop-control]")).toHaveCount(0);
});

for (const path of ["/agents", "/approvals", "/settings/profile", "/audit/decisions"]) {
  test(`${path} sends a signed-out visitor to sign in, and back there after`, async ({ page }) => {
    await page.goto(path);
    const url = new URL(page.url());
    expect(url.pathname).toBe("/login");
    expect(url.searchParams.get("next")).toBe(path);
    await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
    await expect(page.locator("[data-slot=stop-control]")).toHaveCount(0);
  });
}

test("a deep link keeps its query through sign-in", async ({ page }) => {
  await page.goto("/approvals?filter=open");
  expect(new URL(page.url()).searchParams.get("next")).toBe("/approvals?filter=open");
});

test("icons, the manifest and the share image stay public", async ({ request }) => {
  for (const path of ["/favicon.ico", "/favicon.svg", "/apple-touch-icon.png", "/pwa-192.png", "/site.webmanifest", "/og-image.png"]) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(200);
  }
});

test("Continue with Google goes to Supabase's authorize endpoint, returning to the callback with next", async ({ page, baseURL }) => {
  await stubSupabase(page, { "/authorize": { status: 200, contentType: "text/html", body: "<title>Google</title><p>Stub Google" } });
  await page.goto("/login?next=/agents");
  await page.getByRole("button", { name: "Continue with Google" }).click();
  await page.waitForURL(`${STUB_SUPABASE}/auth/v1/authorize**`);
  const authorize = new URL(page.url());
  expect(authorize.searchParams.get("provider")).toBe("google");
  expect(authorize.searchParams.get("redirect_to")).toBe(`${baseURL}/auth/callback?next=%2Fagents`);
  expect(authorize.searchParams.get("code_challenge_method")?.toLowerCase()).toBe("s256");
});

test("next cannot send a visitor off the site", async ({ page, baseURL }) => {
  await stubSupabase(page, { "/authorize": { status: 200, contentType: "text/html", body: "<p>Stub Google" } });
  await page.goto("/login?next=//evil.example/steal");
  await page.getByRole("button", { name: "Continue with Google" }).click();
  await page.waitForURL(`${STUB_SUPABASE}/auth/v1/authorize**`);
  expect(new URL(page.url()).searchParams.get("redirect_to")).toBe(`${baseURL}/auth/callback?next=%2F`);
});

test("a passkey sign-in says so when passkeys are off on the project, as Supabase answers today", async ({ page }) => {
  await stubSupabase(page, {
    "/passkeys/authentication/options": { status: 404, headers: API_VERSION, body: { code: "passkey_disabled", message: "Passkeys are disabled" } },
  });
  await page.goto("/login");
  await page.getByRole("button", { name: "Sign in with a passkey" }).click();
  await expect(page.locator("[data-slot=login-message]")).toHaveText("Passkeys aren’t turned on yet; continue with Google.");
  await expect(page.getByRole("button", { name: "Sign in with a passkey" })).toBeEnabled();
});

test("the email link answers the same whether or not the address can sign in", async ({ page }) => {
  const answers = [
    { status: 200, body: {} },
    { status: 422, body: { code: 422, error_code: "otp_disabled", msg: "Signups not allowed for otp" } },
  ];
  const shown: string[] = [];
  for (const answer of answers) {
    await page.unrouteAll();
    const seen = await stubSupabase(page, { "/otp": answer });
    await page.goto("/login");
    await page.getByRole("textbox", { name: "Or get a sign-in link by email" }).fill("someone@example.com");
    await page.getByRole("button", { name: "Email me a link" }).click();
    const status = page.locator("[data-slot=login-email]").getByRole("status");
    await expect(status).toBeVisible();
    shown.push((await status.textContent()) ?? "");
    expect(seen.map((u) => u.pathname)).toEqual(["/auth/v1/otp"]);
  }
  expect(shown[0]).toContain("If that address can sign in, we’ve sent a link");
  expect(shown[1]).toBe(shown[0]);
});

test("a failed callback comes back to sign in with one generic message", async ({ page }) => {
  await page.goto("/auth/callback?error=access_denied&error_description=Email+not+found");
  expect(new URL(page.url()).pathname).toBe("/login");
  await expect(page.locator("[data-slot=login-message]")).toHaveText("That sign-in didn’t finish. Nothing changed; try again.");
  await expect(page.getByText(/not found/i)).toHaveCount(0);
});
