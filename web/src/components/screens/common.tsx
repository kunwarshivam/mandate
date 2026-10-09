"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { cn } from "@/lib/utils";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Plug } from "pixelarticons/react/Plug.js";
import { BrandOwl } from "@/components/brand/brand-owl";
import { Skeleton } from "@/components/domain/skeleton";
import { clock } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { KEY } from "@/components/kumo/key";

/**
 * The one page grid of every signed-in screen at 64rem and wider (DEC-467): a main column and a 20rem
 * rail beside it. Below 64rem it is one column.
 */
export const PAGE_GRID = "grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,1fr)_20rem] lg:gap-x-14";

/**
 * What needs the owner, on a phone (DEC-482): one row of tinted cards that scrolls sideways, edge to
 * edge, however many there are, so the money below keeps its place. One card spans the row; with
 * more, each takes most of it and the next shows at the edge. From `lg`, a list on hairlines. The strip
 * is `relative`, the containing block of the words a card keeps for screen readers alone: absolutely
 * placed, they would otherwise sit outside the strip's scroll and widen the page by every card that
 * waits off to the right.
 */
export const NEEDS_STRIP =
  "relative grid max-lg:-mx-(--page-x) max-lg:flex max-lg:snap-x max-lg:snap-mandatory max-lg:gap-2 max-lg:overflow-x-auto max-lg:scroll-px-(--page-x) max-lg:px-(--page-x) max-lg:[scrollbar-width:none]";

/** One card in the strip; a hairline row from `lg`. */
export const NEEDS_ITEM = "lg:border-b lg:border-border/70 lg:last:border-b-0 max-lg:flex max-lg:w-full max-lg:shrink-0 max-lg:snap-start max-lg:not-only:w-[min(19rem,85%)]";

/** The card's link on a phone: a tint at least 56px tall. Its words wrap rather than truncate, so a price or a time is never cut. */
export const NEEDS_CARD = "max-lg:mx-0 max-lg:w-full max-lg:items-center max-lg:rounded-xl max-lg:px-3 max-lg:py-2";

export function Section({ title, action, children, className, id }: { title: string; action?: ReactNode; children: ReactNode; className?: string; id?: string }) {
  const headingId = id ?? `section-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  return (
    <section aria-labelledby={headingId} className={cn("grid grid-cols-1 content-start gap-(--block-gap)", className)}>
      <div className="flex min-h-8 items-baseline justify-between gap-3">
        <h2 id={headingId} className="text-h2">
          {title}
        </h2>
        {action}
      </div>
      {children}
    </section>
  );
}

/** A quiet link beside a section heading: to the full list behind it. */
export function SectionLink({ href, children, className }: { href: string; children: ReactNode; className?: string }) {
  return (
    <Link
      href={href}
      className={cn(
        "-mx-2 inline-flex min-h-9 items-center gap-1 rounded-md px-2 text-sm font-medium text-lapis outline-none hover:bg-lapis-soft focus-visible:ring-2 focus-visible:ring-ring max-lg:min-h-11",
        className,
      )}
    >
      {children}
      <ArrowRight aria-hidden className="size-6" />
    </Link>
  );
}

/** Content without a box: the page is the surface. `well` recesses it for a group that needs one. */
export function Panel({ children, className, well = false }: { children: ReactNode; className?: string; well?: boolean }) {
  return <div className={cn(well && "rounded-2xl bg-background px-4 py-3 sm:px-5", className)}>{children}</div>;
}

/**
 * Silence as a feature: the brand owl and one plain line, in the landing page's chrome face, with no
 * meaning colour and no motion. Home's Needs you and the top of Alerts say it on the condition
 * `nothingNeedsYou` decides, so they say it together or not at all (C-13).
 */
export function AllClear() {
  return (
    <p data-slot="all-clear" className="pixel-face flex min-h-11 items-center gap-3 text-sm text-muted-foreground">
      <BrandOwl className="size-8" />
      All clear. Nothing needs you.
    </p>
  );
}

/** No agents yet: one sentence and the one next step. */
export function EmptyBoard() {
  return (
    <section data-slot="empty" aria-labelledby="empty-title" className="reveal grid max-w-xl content-start gap-5 pt-6 sm:pt-12">
      <BrandOwl className="size-20" />
      <h1 id="empty-title" className="pixel-face text-h1">
        No agents yet
      </h1>
      <p className="max-w-measure text-lg text-muted-foreground">An agent trades on paper within a mandate you describe and confirm, field by field.</p>
      <Link
        href="/agents/new"
        className={cn("w-fit", KEY)}
      >
        Describe your first agent <ArrowRight aria-hidden className="size-6" />
      </Link>
    </section>
  );
}

/** The deployment did not answer: no agent data is shown and nothing is kept on this device. */
export function UnreachableNotice() {
  const { ws } = useRuntime();
  return (
    <div role="alert" data-slot="unreachable-notice" className="reveal grid max-w-2xl gap-4 pt-6 text-foreground sm:pt-12">
      <Plug aria-hidden className="-ml-2.5 size-12 text-muted-foreground" />
      <h1 className="text-h1">Cannot reach your deployment</h1>
      <div className="grid max-w-measure gap-3 text-base">
        <p>
          It last answered at {clock(ws.health.deployment.as_of)}. No agent data is shown, and none is kept on this device, so nothing here can be out of date.
        </p>
        <p className="text-muted-foreground">
          Agents keep running on the deployment, and protective orders rest at the broker. The Stop control still opens; from here it cannot deliver a request, so it tells
          you how to reach the broker directly.
        </p>
      </div>
    </div>
  );
}

export function ScreenSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <div className="grid gap-(--section-gap)" aria-busy="true" aria-label="Loading" data-slot="skeleton">
      <div className={PAGE_GRID}>
        <div className="grid gap-5">
          <div className="grid gap-2.5">
            <Skeleton className="h-4 w-28 rounded-sm" />
            <Skeleton className="h-12 w-60 rounded-lg" />
            <Skeleton className="h-4 w-44 rounded-sm" />
          </div>
          <Skeleton className="h-56 rounded-xl sm:h-64" />
        </div>
        <div className="grid content-start gap-3 lg:pt-1">
          <Skeleton className="h-6 w-36 rounded-sm" />
          <Skeleton className="h-36 rounded-2xl" />
        </div>
      </div>
      <div className="grid gap-3">
        <Skeleton className="h-6 w-24 rounded-sm" />
        {Array.from({ length: rows }, (_, i) => (
          <Skeleton key={i} className="h-16 rounded-xl" />
        ))}
      </div>
      <span className="sr-only">Loading from your deployment. Values appear once they arrive; nothing from an earlier visit is shown.</span>
    </div>
  );
}

/** Loading shows a skeleton and never earlier values; unreachable shows no data at all. */
export function WorkspaceGate({ children, skeleton }: { children: ReactNode; skeleton?: ReactNode }) {
  const { ws } = useRuntime();
  if (ws.status === "loading") return <>{skeleton ?? <ScreenSkeleton />}</>;
  if (ws.status === "unreachable") return <UnreachableNotice />;
  return <>{children}</>;
}
