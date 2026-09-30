"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { cn } from "@/lib/utils";
import { ArrowRight, Plugs } from "@phosphor-icons/react";
import { Owl } from "@/components/domain/owl";
import { Skeleton } from "@/components/domain/skeleton";
import { clock } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";

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
      <ArrowRight aria-hidden className="size-3.5" />
    </Link>
  );
}

/** Content without a box: the page is the surface. `well` recesses it for a group that needs one. */
export function Panel({ children, className, well = false }: { children: ReactNode; className?: string; well?: boolean }) {
  return <div className={cn(well && "rounded-2xl bg-background px-4 py-3 sm:px-5", className)}>{children}</div>;
}

/** No agents yet: one sentence and the one next step. */
export function EmptyBoard() {
  return (
    <section data-slot="empty" aria-labelledby="empty-title" className="reveal grid max-w-xl content-start gap-5 pt-6 sm:pt-12">
      <Owl seed="owlhead" mood="awake" className="size-20" />
      <h1 id="empty-title" className="text-h1">
        No agents yet
      </h1>
      <p className="max-w-measure text-lg text-muted-foreground">An agent trades on paper within a mandate you describe and confirm, field by field.</p>
      <Link
        href="/agents/new"
        className="press inline-flex h-12 w-fit items-center gap-2 rounded-lg bg-lapis pr-5 pl-6 font-semibold text-lapis-foreground outline-none hover:bg-lapis-strong focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
      >
        Describe your first agent <ArrowRight aria-hidden className="size-4" />
      </Link>
    </section>
  );
}

/** The deployment did not answer: no agent data is shown and nothing is kept on this device. */
export function UnreachableNotice() {
  const { ws } = useRuntime();
  return (
    <div role="alert" data-slot="unreachable-notice" className="reveal grid max-w-2xl gap-4 pt-6 text-foreground sm:pt-12">
      <Plugs aria-hidden className="size-7 text-muted-foreground" />
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
      <div className="grid grid-cols-1 gap-(--section-gap) lg:grid-cols-[minmax(0,1fr)_20rem] lg:gap-x-14">
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
