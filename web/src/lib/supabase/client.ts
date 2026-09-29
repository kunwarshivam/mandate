import { createBrowserClient } from "@supabase/ssr";
import { SUPABASE_PUBLISHABLE_KEY, SUPABASE_URL } from "@/lib/auth-config";

/**
 * The browser's Supabase client. `@supabase/ssr` keeps the session in cookies, which the proxy and
 * the server read too, and makes one client per page. Passkeys are a beta API that must be opted
 * into (DEC-211). Call it only while `authEnabled`.
 */
export function createClient() {
  return createBrowserClient(SUPABASE_URL, SUPABASE_PUBLISHABLE_KEY, {
    auth: { experimental: { passkey: true } },
  });
}

export type BrowserClient = ReturnType<typeof createClient>;
