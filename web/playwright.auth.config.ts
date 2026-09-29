import { defineConfig, devices } from "@playwright/test";
import { STUB_SUPABASE } from "./e2e-auth/stub";
import { AUTH_E2E_FLAG } from "./scripts/e2e-build.mjs";

const PORT = Number(process.env.OWLHEAD_AUTH_E2E_PORT ?? 4318);
const ORIGIN = `http://127.0.0.1:${PORT}`;

/**
 * Sign-in on, signed out (DEC-211): a production build with the Supabase variables set to a stub
 * address, in `.next-auth-e2e`. A signed-out request never reaches Supabase from the server (with no
 * session cookie there are no claims to verify), so the proxy's decisions are real; what the browser
 * asks of Supabase is answered by the specs. Signing in is not covered: the server would verify a
 * session against the project's keys, and Playwright cannot answer the server's requests.
 */
export default defineConfig({
  testDir: "e2e-auth",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: ORIGIN,
    trace: "retain-on-failure",
  },
  webServer: {
    command: `npm run build && npx next start --hostname 127.0.0.1 --port ${PORT}`,
    env: {
      [AUTH_E2E_FLAG]: "1",
      NEXT_PUBLIC_SUPABASE_URL: STUB_SUPABASE,
      NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY: "sb_publishable_e2e_stub",
      NEXT_PUBLIC_OWLHEAD_EMAIL_SIGNIN: "1",
    },
    url: ORIGIN,
    reuseExistingServer: false,
    timeout: 300_000,
  },
});
