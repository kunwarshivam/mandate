"use client";

import { type AccessToken, type Locks, type SessionKeeper, createSessionKeeper } from "@/api/session";
import { authEnabled } from "./auth-config";
import { tokenFromStub } from "./stub-sign-in";
import { createClient } from "./supabase/client";

/** What the sign-in client's `getSession` answers, as far as the API session needs it. */
interface SignInAnswer {
  data: { session: { access_token: string } | null };
  error: { status?: number; name?: string } | null;
}

/**
 * The access token to trade for an API session: the sign-in client's own, or none when signed out. A
 * provider that does not answer is `unreachable`, which the API reads as an outage (§3.11 item 6).
 */
export async function tokenFrom(getSession: () => Promise<SignInAnswer>): Promise<AccessToken> {
  try {
    const { data, error } = await getSession();
    if (error) return error.status === undefined || error.status === 0 || error.status >= 500 ? "unreachable" : null;
    return data.session?.access_token ?? null;
  } catch {
    return "unreachable";
  }
}

const CHANNEL = "owlhead-api-session";

/** Where the token traded for the API session comes from: Supabase's sign-in client, or a dev cell's stub issuer (DEC-2406). */
export type SignInSource = "supabase" | "stub";

/**
 * The browser's keeper of the API session, or `null` while sign-in is off (DEC-211) and no stub
 * sign-in is configured: the fixtures and the e2e build's mock API need none. Tabs share refreshes
 * through `navigator.locks` and tell each other when one refreshed through a `BroadcastChannel`;
 * neither stores anything.
 */
export function browserSessionKeeper(origin: string, stubSignIn = false): SessionKeeper | null {
  if (typeof window === "undefined") return null;
  const signIn: SignInSource | null = stubSignIn ? "stub" : authEnabled ? "supabase" : null;
  if (!signIn) return null;
  const accessToken = signIn === "stub" ? () => tokenFromStub((input, init) => window.fetch(input, init)) : supabaseToken();
  const locks = (navigator as Navigator & { locks?: Locks }).locks;
  let latest = 0;
  const channel = typeof BroadcastChannel === "undefined" ? null : new BroadcastChannel(CHANNEL);
  if (channel) channel.onmessage = (event: MessageEvent<unknown>) => void (typeof event.data === "number" && event.data > latest && (latest = event.data));
  return createSessionKeeper({
    origin,
    fetch: (input, init) => window.fetch(input, init),
    accessToken,
    ...(locks ? { locks } : {}),
    channel: {
      post: (at) => {
        latest = at;
        channel?.postMessage(at);
      },
      latest: () => latest,
    },
  });
}

function supabaseToken(): () => Promise<AccessToken> {
  const supabase = createClient();
  return () => tokenFrom(() => supabase.auth.getSession());
}
