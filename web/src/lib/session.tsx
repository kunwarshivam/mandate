"use client";

import { type ReactNode, createContext, useContext } from "react";
import { type BrowserClient, createClient } from "@/lib/supabase/client";

/** The signed-in person as the server verified them; the fixture role stays separate from it (DEC-211). */
export interface Session {
  email: string | null;
}

const SessionContext = createContext<Session | null>(null);

/** Null while sign-in is off or no one is signed in, and then the shell shows no account line or Sign out. */
export function SessionProvider({ session, children }: { session: Session | null; children: ReactNode }) {
  return <SessionContext.Provider value={session}>{children}</SessionContext.Provider>;
}

export function useSession(): Session | null {
  return useContext(SessionContext);
}

/**
 * Ends the session and goes to `/`, which the proxy then serves as the welcome page. The page
 * reloads so no screen keeps the signed-in state in memory.
 */
export async function signOut(
  auth: Pick<BrowserClient["auth"], "signOut"> = createClient().auth,
  navigate: (href: string) => void = (href) => window.location.assign(href),
) {
  await auth.signOut();
  navigate("/");
}
