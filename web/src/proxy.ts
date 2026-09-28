import { type NextRequest, NextResponse } from "next/server";
import { isScenario } from "@/fixtures/workspace";
import { SCENARIO_COOKIE, SCENARIO_PARAM, scenariosEnabled } from "@/lib/scenario";

/** Dev only: `?scenario=stale` sets the scenario cookie, then drops the parameter from the URL. */
export function proxy(request: NextRequest) {
  const requested = request.nextUrl.searchParams.get(SCENARIO_PARAM);
  if (!scenariosEnabled || requested === null) return NextResponse.next();
  const url = request.nextUrl.clone();
  url.searchParams.delete(SCENARIO_PARAM);
  const response = NextResponse.redirect(url);
  if (isScenario(requested)) {
    response.cookies.set(SCENARIO_COOKIE, requested, { path: "/", sameSite: "strict" });
  }
  return response;
}

export const config = {
  matcher: ["/((?!_next/|favicon.ico|icon.svg).*)"],
};
