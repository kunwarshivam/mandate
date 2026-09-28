/** The dev-only scenario switch. Outside `next dev` the app always renders the normal fixture. */
export const SCENARIO_COOKIE = "mandate-scenario";
export const SCENARIO_PARAM = "scenario";

export const scenariosEnabled = process.env.NODE_ENV === "development";
