/**
 * The fixture scenario switch (`?scenario=stale` and its cookie). It is on in `next dev`, and in the e2e
 * build, whose `OWLHEAD_E2E_SCENARIOS=1` next.config.ts inlines at build time (`scripts/e2e-build.mjs`).
 * A production build always renders the normal fixture. The switcher panel is `next dev` only, and a
 * production build does not contain it (`devOnlyAliases`).
 */
export const SCENARIO_COOKIE = "mandate-scenario";
export const SCENARIO_PARAM = "scenario";

export function scenariosOn(nodeEnv: string | undefined, e2eFlag: string | undefined): boolean {
  return nodeEnv === "development" || e2eFlag === "1";
}

export const scenariosEnabled = scenariosOn(process.env.NODE_ENV, process.env.OWLHEAD_E2E_SCENARIOS);

export const scenarioSwitcherShown = process.env.NODE_ENV === "development";
