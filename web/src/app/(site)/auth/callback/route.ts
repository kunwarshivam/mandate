import type { NextRequest } from "next/server";
import { authEnabled } from "@/lib/auth-config";
import { LOGIN_PATH, PASSKEY_PATH, safeNext } from "@/lib/auth-routes";
import { createClient } from "@/lib/supabase/server";

/**
 * A relative location, so the browser stays on the host it signed in on (Next spells a loopback host
 * `localhost` in `request.url`, and a session cookie set for 127.0.0.1 would not follow it there).
 */
function goTo(path: string) {
  return new Response(null, { status: 303, headers: { Location: path, "Cache-Control": "private, no-store" } });
}

/**
 * Where Google and an email link return (DEC-211). The code becomes a session in cookies; then the
 * user goes on to `next`, by way of the passkey step if they have none yet. Any failure goes back to
 * sign-in with one generic message.
 */
export async function GET(request: NextRequest) {
  const params = request.nextUrl.searchParams;
  const next = safeNext(params.get("next"));
  const code = params.get("code");
  if (!authEnabled || !code || params.has("error")) return goTo(`${LOGIN_PATH}?error=1`);

  const supabase = await createClient();
  const { error } = await supabase.auth.exchangeCodeForSession(code);
  if (error) return goTo(`${LOGIN_PATH}?error=1`);

  const { data: passkeys, error: listError } = await supabase.auth.passkey.list();
  if (!listError && passkeys?.length === 0) return goTo(`${PASSKEY_PATH}?next=${encodeURIComponent(next)}`);
  return goTo(next);
}
