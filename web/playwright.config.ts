import { defineConfig, devices } from "@playwright/test";

/**
 * Computed styles in a real browser, against the production build (`next build`, then `next start`).
 * One browser, Chromium, to keep CI minutes low, in light and in dark.
 */
export default defineConfig({
  testDir: "e2e",
  globalSetup: "./e2e/global-setup.mjs",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  use: {
    baseURL: "http://127.0.0.1:4317",
    trace: "retain-on-failure",
  },
  /** Every spec runs in both themes: the theme follows the system until the owner picks one. */
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"], colorScheme: "light" } },
    { name: "chromium-dark", use: { ...devices["Desktop Chrome"], colorScheme: "dark" } },
  ],
  webServer: {
    command: "npm run start",
    url: "http://127.0.0.1:4317",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
