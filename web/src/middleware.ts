import { type NextRequest, NextResponse } from "next/server";
import { isScenario } from "@/fixtures/workspace";
import { authEnabled } from "@/lib/auth-config";
import { authRoute } from "@/lib/auth-routes";
import { CVD_COOKIE, CVD_PARAM, colourBlindEnabled, cvdFromParam } from "@/lib/colour-pref";
import { SCENARIO_COOKIE, SCENARIO_PARAM, scenariosEnabled } from "@/lib/scenario";
import { updateSession, withSession } from "@/lib/supabase/proxy";

/** Dev only: `?scenario=stale` and `?cvd=1` set their cookies, then drop the parameters from the URL. */
function preferenceRedirect(request: NextRequest): NextResponse | null {
  const params = request.nextUrl.searchParams;
  const scenario = scenariosEnabled ? params.get(SCENARIO_PARAM) : null;
  const cvd = colourBlindEnabled ? params.get(CVD_PARAM) : null;
  if (scenario === null && cvd === null) return null;
  const url = request.nextUrl.clone();
  for (const name of [SCENARIO_PARAM, CVD_PARAM]) url.searchParams.delete(name);
  const response = NextResponse.redirect(url);
  const set = (name: string, value: string) => response.cookies.set(name, value, { path: "/", sameSite: "strict" });
  if (isScenario(scenario)) set(SCENARIO_COOKIE, scenario);
  if (cvd !== null) set(CVD_COOKIE, cvdFromParam(cvd));
  return response;
}

/**
 * The edge entry point that replaced `src/proxy.ts` on Cloudflare Workers (DEC-823, DEC-731 item 1):
 * OpenNext runs edge middleware, not Node's proxy, so this file imports nothing from `node:` and
 * sets no `runtime`. With sign-in off (`authEnabled`, DEC-211) this is the dev preference switch alone. With it on, it
 * also refreshes the Supabase session and applies `authRoute`: a signed-out visitor sees the welcome
 * page at `/` and is sent to sign in from any other screen.
 */
export async function middleware(request: NextRequest): Promise<NextResponse> {
  const preference = preferenceRedirect(request);
  if (!authEnabled) return preference ?? NextResponse.next();
  const { response, signedIn } = await updateSession(request);
  if (preference) return withSession(response, preference);
  const route = authRoute(request.nextUrl, signedIn);
  switch (route.kind) {
    case "pass":
      return response;
    case "rewrite":
      return withSession(response, NextResponse.rewrite(new URL(route.to, request.url), { request: { headers: request.headers } }));
    case "redirect":
      return withSession(response, NextResponse.redirect(new URL(route.to, request.url)));
    default: {
      const unhandled: never = route;
      return unhandled;
    }
  }
}

export const config = {
  matcher: ["/((?!_next/|art/|video/|favicon|apple-touch-icon\\.png|pwa-|og-image\\.png|site\\.webmanifest|robots\\.txt|push-sw\\.js).*)"],
};
