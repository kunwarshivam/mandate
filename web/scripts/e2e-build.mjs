// The e2e build. `OWLHEAD_E2E_SCENARIOS=1` at build time turns the fixture scenario switch
// (`?scenario=stale`) on for the Playwright suite, which runs against a production build. next.config.ts
// inlines the flag, so it is fixed when the app is built, and writes that build to its own directory:
// the production build in `.next` never accepts a scenario, whatever the environment says when it starts.

export const E2E_FLAG = "OWLHEAD_E2E_SCENARIOS";

/** @param {Record<string, string | undefined>} env */
export function isE2eBuild(env = process.env) {
  return env[E2E_FLAG] === "1";
}

/** @param {Record<string, string | undefined>} env */
export function distDir(env = process.env) {
  return isE2eBuild(env) ? ".next-e2e" : ".next";
}
