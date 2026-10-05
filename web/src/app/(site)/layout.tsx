import type { ReactNode } from "react";

/**
 * The public pages' frame: no header, because each page draws its own window on the landing page's
 * desktop (DEC-213, DEC-469), and none of the app's controls, because nothing here acts on an account.
 */
export default function SiteLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-dvh flex-col bg-card text-foreground">
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      {children}
    </div>
  );
}
