import type { ReactNode } from "react";

/** The centred column a public page other than the landing page sits in; the landing page brings its own `<main>`. */
export function SiteMain({ children }: { children: ReactNode }) {
  return (
    <main id="main" tabIndex={-1} className="mx-auto flex w-full max-w-(--content-max) flex-1 flex-col px-(--page-x) pt-(--page-top) pb-(--page-bottom) outline-none">
      {children}
    </main>
  );
}
