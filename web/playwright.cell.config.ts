import { defineConfig, devices } from "@playwright/test";

/**
 * The cell demo's web gestures (lane W2, DEC-2406), run by `xtask/src/cell/web.rs` against a running
 * local cell, never by `npm run test:e2e`. The driver passes every setting: the API's origin and
 * workspace and the stub sign-in (`web_run`), a certificate for `next dev`'s https, and where the
 * gesture writes what it recorded. This config starts `next dev` at the API's app origin and stops it.
 */
const ORIGIN = "https://localhost:4317";
const KEY = process.env.OWLHEAD_CELL_TLS_KEY ?? "";
const CERT = process.env.OWLHEAD_CELL_TLS_CERT ?? "";

export default defineConfig({
  testDir: "e2e-cell",
  fullyParallel: false,
  forbidOnly: true,
  retries: 0,
  workers: 1,
  globalTimeout: 90_000,
  reporter: "list",
  use: { baseURL: ORIGIN, ignoreHTTPSErrors: true, trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], colorScheme: "light" } }],
  webServer: {
    command: `npx next dev --experimental-https --experimental-https-key ${KEY} --experimental-https-cert ${CERT} --hostname localhost --port 4317`,
    url: ORIGIN,
    ignoreHTTPSErrors: true,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
