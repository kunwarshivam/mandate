import { type NextRequest, NextResponse } from "next/server";
import { isScenario } from "@/fixtures/workspace";
import { CVD_COOKIE, CVD_PARAM, colourBlindEnabled, cvdFromParam } from "@/lib/colour-pref";
import { SCENARIO_COOKIE, SCENARIO_PARAM, scenariosEnabled } from "@/lib/scenario";

/** Dev only: `?scenario=stale` and `?cvd=1` set their cookies, then drop the parameters from the URL. */
export function proxy(request: NextRequest) {
  const params = request.nextUrl.searchParams;
  const scenario = scenariosEnabled ? params.get(SCENARIO_PARAM) : null;
  const cvd = colourBlindEnabled ? params.get(CVD_PARAM) : null;
  if (scenario === null && cvd === null) return NextResponse.next();
  const url = request.nextUrl.clone();
  for (const name of [SCENARIO_PARAM, CVD_PARAM]) url.searchParams.delete(name);
  const response = NextResponse.redirect(url);
  const set = (name: string, value: string) => response.cookies.set(name, value, { path: "/", sameSite: "strict" });
  if (isScenario(scenario)) set(SCENARIO_COOKIE, scenario);
  if (cvd !== null) set(CVD_COOKIE, cvdFromParam(cvd));
  return response;
}

export const config = {
  matcher: ["/((?!_next/|favicon.ico).*)"],
};
