import { defineConfig, devices } from "@playwright/test";

/**
 * The cell demo's web gestures (lane W2, DEC-2406), run by `xtask/src/cell/web.rs` against a running
 * local cell, never by `npm run test:e2e`. The driver passes every setting: the API's origin and
 * workspace and the stub sign-in (`web_run`), a certificate for `next dev`'s https, and where the
 * gesture writes what it recorded. This config starts the API's https front (`e2e-cell/tls-proxy.mjs`,
 * so the session cookie is same-site with the page) and `next dev` at the API's app origin, and stops
 * both.
 */
const ORIGIN = "https://localhost:4317";
const KEY = process.env.OWLHEAD_CELL_TLS_KEY ?? "";
const CERT = process.env.OWLHEAD_CELL_TLS_CERT ?? "";
/** The API as the page reaches it, the https front `e2e-cell/tls-proxy.mjs` serves, and what it forwards to. */
const API = process.env.OWLHEAD_API_ORIGIN ?? "";
const UPSTREAM = process.env.OWLHEAD_CELL_API_UPSTREAM ?? "";

/**
 * Which gesture this run is: the kill switch by default, as the demo's step 7 has always run, or the
 * file the driver names (`trade-loop.spec.ts` for the end-to-end loop). One run is one gesture.
 */
const SPEC = process.env.OWLHEAD_CELL_SPEC ?? "kill-switch.spec.ts";
/** The whole run's bound; the trade loop waits on the runtime, the host's confirmation and a fill. */
const TIMEOUT_MS = Number(process.env.OWLHEAD_CELL_TIMEOUT_MS ?? 90_000);
/**
 * Whether the driver started this cell waiving a first passkey's cool-off (DEC-2146): then, and only
 * then, `next dev` builds web/ to try a passkey still in its cool-off and leave the verdict to the
 * server (DEC-2411). This config is the one place that sets the flag; a test refuses any other.
 */
const COOL_OFF_WAIVED = process.env.OWLHEAD_CELL_COOL_OFF_WAIVED === "1";

export default defineConfig({
  testDir: "e2e-cell",
  testMatch: SPEC,
  fullyParallel: false,
  forbidOnly: true,
  retries: 0,
  workers: 1,
  globalTimeout: TIMEOUT_MS,
  reporter: "list",
  use: { baseURL: ORIGIN, ignoreHTTPSErrors: true, trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], colorScheme: "light" } }],
  webServer: [
    {
      command: `node e2e-cell/tls-proxy.mjs ${API} ${UPSTREAM} ${KEY} ${CERT}`,
      port: Number(new URL(API || "https://localhost:0").port),
      reuseExistingServer: false,
      timeout: 10_000,
    },
    {
      command: `npx next dev --experimental-https --experimental-https-key ${KEY} --experimental-https-cert ${CERT} --hostname localhost --port 4317`,
      env: { NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED: COOL_OFF_WAIVED ? "1" : "0" },
      url: ORIGIN,
      ignoreHTTPSErrors: true,
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
});
