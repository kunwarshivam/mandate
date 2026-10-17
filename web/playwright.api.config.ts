import { defineConfig, devices } from "@playwright/test";
import { E2E_FLAG } from "./scripts/e2e-build.mjs";

/**
 * The app with its data source set to the workspace API (DEC-975), against the e2e build. The server
 * is configured as a deployment would be, by environment at start, never at build: the build is the
 * one `playwright.config.ts` makes. Until the API server (E21-31) is on `main`, the API it calls is the
 * e2e build's own fixture-backed mock at `/v1` (`src/app/v1/`), which a production build does not serve.
 */
const PORT = Number(process.env.OWLHEAD_API_E2E_PORT ?? 4319);
const ORIGIN = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  testDir: "e2e-api",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  use: { baseURL: ORIGIN, trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], colorScheme: "light" } }],
  webServer: {
    command: `npm run build && npx next start --hostname 127.0.0.1 --port ${PORT}`,
    env: {
      [E2E_FLAG]: "1",
      OWLHEAD_DATA_SOURCE: "api",
      OWLHEAD_API_ORIGIN: process.env.OWLHEAD_API_E2E_ORIGIN ?? ORIGIN,
      OWLHEAD_WORKSPACE_ID: process.env.OWLHEAD_API_E2E_WORKSPACE ?? "ws_01J9ZQ4M0000000000000000AB",
    },
    url: ORIGIN,
    reuseExistingServer: false,
    timeout: 300_000,
  },
});
