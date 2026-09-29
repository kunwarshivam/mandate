// The e2e build. `OWLHEAD_E2E_SCENARIOS=1` at build time turns the fixture scenario switch
// (`?scenario=stale`) on for the Playwright suite, which runs against a production build. next.config.ts
// inlines the flag, so it is fixed when the app is built, and writes that build to its own directory:
// the production build in `.next` never accepts a scenario, whatever the environment says when it starts.
// `OWLHEAD_AUTH_E2E=1` marks the sign-in suite's build (`playwright.auth.config.ts`), which has sign-in
// on against a stub Supabase address; it too gets its own directory, so `.next` never holds it.

export const E2E_FLAG = "OWLHEAD_E2E_SCENARIOS";
export const AUTH_E2E_FLAG = "OWLHEAD_AUTH_E2E";

/** @param {Record<string, string | undefined>} env */
export function isE2eBuild(env = process.env) {
  return env[E2E_FLAG] === "1";
}

/** @param {Record<string, string | undefined>} env */
export function isAuthE2eBuild(env = process.env) {
  return env[AUTH_E2E_FLAG] === "1";
}

/** @param {Record<string, string | undefined>} env */
export function distDir(env = process.env) {
  if (isE2eBuild(env)) return ".next-e2e";
  return isAuthE2eBuild(env) ? ".next-auth-e2e" : ".next";
}
