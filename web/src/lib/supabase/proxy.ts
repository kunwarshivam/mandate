import { createServerClient } from "@supabase/ssr";
import { type NextRequest, NextResponse } from "next/server";
import { SUPABASE_PUBLISHABLE_KEY, SUPABASE_URL } from "@/lib/auth-config";

const CACHE_HEADERS = ["cache-control", "expires", "pragma"] as const;

/**
 * Refreshes the Supabase session for the proxy, the pattern of the `@supabase/ssr` Next.js guide:
 * refreshed cookies go onto the request, for the server components, and onto the response, for the
 * browser, with the cache headers that keep a shared cache from storing one person's session.
 * `getClaims()` verifies the token's signature, so a forged cookie counts as signed out; the session
 * as the cookie states it, unverified, never decides.
 */
export async function updateSession(request: NextRequest): Promise<{ response: NextResponse; signedIn: boolean }> {
  let response = NextResponse.next({ request });
  const supabase = createServerClient(SUPABASE_URL, SUPABASE_PUBLISHABLE_KEY, {
    cookies: {
      getAll() {
        return request.cookies.getAll();
      },
      setAll(cookiesToSet, headers) {
        for (const { name, value } of cookiesToSet) request.cookies.set(name, value);
        response = NextResponse.next({ request });
        for (const { name, value, options } of cookiesToSet) response.cookies.set(name, value, options);
        for (const [key, value] of Object.entries(headers)) response.headers.set(key, value);
      },
    },
  });
  // Nothing may run between creating the client and getClaims(), or the refresh can be lost.
  const { data } = await supabase.auth.getClaims();
  const signedIn = Boolean(data?.claims?.sub) && data?.claims?.is_anonymous !== true;
  return { response, signedIn };
}

/** A redirect or rewrite must carry the refreshed cookies and cache headers, or the session is lost. */
export function withSession(session: NextResponse, to: NextResponse): NextResponse {
  for (const cookie of session.cookies.getAll()) to.cookies.set(cookie);
  for (const header of CACHE_HEADERS) {
    const value = session.headers.get(header);
    if (value) to.headers.set(header, value);
  }
  return to;
}
