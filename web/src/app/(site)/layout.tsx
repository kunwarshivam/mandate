import type { ReactNode } from "react";
import { cookies } from "next/headers";
import { DesktopStyleProvider } from "@/components/site/desktop-style";
import { DESKTOP_COOKIE, desktopStyleFrom } from "@/lib/desktop-style";

/**
 * The public pages' frame: no header, because each page draws its own window on the landing page's
 * desktop (DEC-213, DEC-469), and none of the app's controls, because nothing here acts on an account.
 * It wears the desktop the visitor chose, Windows or Mac, from its cookie (DEC-904).
 */
export default async function SiteLayout({ children }: { children: ReactNode }) {
  const style = desktopStyleFrom((await cookies()).get(DESKTOP_COOKIE)?.value);
  return (
    <DesktopStyleProvider initial={style} className="flex min-h-dvh flex-col bg-card text-foreground">
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      {children}
    </DesktopStyleProvider>
  );
}
