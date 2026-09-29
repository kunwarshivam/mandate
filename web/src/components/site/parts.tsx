import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** The landing page's column: the app's content width and page padding. */
export const COLUMN = "mx-auto w-full max-w-(--content-max) px-(--page-x)";

const FOCUS = "outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card";

/** The one filled action: the ink pill the app uses for its primary buttons. */
export const PRIMARY_LINK = cn(
  "press inline-flex h-11 items-center rounded-full bg-primary px-6 text-sm font-semibold text-primary-foreground hover:bg-lapis-strong",
  FOCUS,
);

export const SECONDARY_LINK = cn("press inline-flex h-11 items-center rounded-full border border-border px-6 text-sm font-semibold text-foreground hover:bg-background", FOCUS);

export const TEXT_LINK = cn("rounded-sm font-medium text-foreground underline decoration-border underline-offset-4 hover:decoration-current", FOCUS);

/** A section with a real heading, on the page's column, set apart by a hairline and whitespace. */
export function SiteSection({ id, title, lead, children, className }: { id: string; title: string; lead?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className={cn(COLUMN, "scroll-mt-24", className)}>
      <div className="grid gap-10 border-t border-border/70 pt-14 pb-16 sm:pt-16 lg:gap-12 lg:pt-20 lg:pb-24">
        <div className="grid max-w-measure gap-3">
          <h2 id={`${id}-title`} className="text-h1 text-balance">
            {title}
          </h2>
          {lead ? <p className="text-lg text-pretty text-muted-foreground">{lead}</p> : null}
        </div>
        {children}
      </div>
    </section>
  );
}

/** Marks what the docs describe but the product does not do yet. */
export function NotYet({ children = "Coming later" }: { children?: ReactNode }) {
  return (
    <span data-slot="not-yet" className="inline-flex h-6 w-fit items-center rounded-full border border-dashed border-muted-foreground px-2.5 text-label text-muted-foreground">
      {children}
    </span>
  );
}
