import { createServerClient } from "@supabase/ssr";
import { cookies } from "next/headers";
import { SUPABASE_PUBLISHABLE_KEY, SUPABASE_URL, authEnabled } from "@/lib/auth-config";

/** A Supabase client for one server request (a server component or a route handler), on its cookies. */
export async function createClient() {
  const cookieStore = await cookies();
  return createServerClient(SUPABASE_URL, SUPABASE_PUBLISHABLE_KEY, {
    auth: { experimental: { passkey: true } },
    cookies: {
      getAll() {
        return cookieStore.getAll();
      },
      setAll(cookiesToSet) {
        try {
          for (const { name, value, options } of cookiesToSet) cookieStore.set(name, value, options);
        } catch {
          // A server component cannot write cookies; the proxy has already refreshed the session.
        }
      },
    },
  });
}

export interface SignedInUser {
  email: string | null;
}

/**
 * Who is signed in, from claims whose signature `getClaims()` verified, never from the unverified
 * session in the cookie. Null when sign-in is off or no one is signed in.
 */
export async function signedInUser(): Promise<SignedInUser | null> {
  if (!authEnabled) return null;
  const supabase = await createClient();
  const { data } = await supabase.auth.getClaims();
  const claims = data?.claims;
  if (!claims?.sub || claims.is_anonymous === true) return null;
  return { email: typeof claims.email === "string" && claims.email !== "" ? claims.email : null };
}
