import type { ReactNode } from "react";
import { SiteHeader } from "@/components/site-frame/site-header";
import { signedInUser } from "@/lib/supabase/server";

/** The public pages' frame: the brand and "Sign in", on the same tokens, type and themes as the app. */
export default async function SiteLayout({ children }: { children: ReactNode }) {
  const signedIn = (await signedInUser()) !== null;
  return (
    <div className="flex min-h-dvh flex-col bg-card text-foreground">
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      <SiteHeader signedIn={signedIn} />
      {children}
    </div>
  );
}
