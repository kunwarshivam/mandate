"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { cn } from "@/lib/utils";
import { ArrowRight, Unplug } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Skeleton } from "@/components/ui/skeleton";
import { clock } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";

export function PageHeader({ title, lead, children, className }: { title: string; lead?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <header className={cn("mb-(--block-gap) flex flex-wrap items-end justify-between gap-3", className)}>
      <div className="grid gap-1">
        <h1 className="text-title sm:text-display">{title}</h1>
        {lead ? <p className="max-w-prose text-muted-foreground">{lead}</p> : null}
      </div>
      {children}
    </header>
  );
}

export function Section({ title, action, children, className, id }: { title: string; action?: ReactNode; children: ReactNode; className?: string; id?: string }) {
  const headingId = id ?? `section-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  return (
    <section aria-labelledby={headingId} className={cn("grid grid-cols-1 content-start gap-(--block-gap)", className)}>
      <div className="flex items-baseline justify-between gap-3 border-b-2 border-foreground pb-1.5">
        <h2 id={headingId} className="text-heading">
          {title}
        </h2>
        {action}
      </div>
      {children}
    </section>
  );
}

export function Panel({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("bg-card p-3 sm:p-4", className)}>{children}</div>;
}

/** No agents yet: the account board, empty, with the one next step. */
export function EmptyBoard() {
  return (
    <section data-slot="empty" aria-labelledby="empty-title" className="reveal grid max-w-3xl content-start gap-4 bg-lapis p-4 text-lapis-foreground sm:p-6">
      <h1 id="empty-title" className="text-title sm:text-display">
        No agents yet
      </h1>
      <p className="max-w-prose text-lapis-muted">An agent trades on paper within a mandate you describe and confirm, field by field.</p>
      <Link
        href="/agents/new"
        className="press inline-flex h-11 w-fit items-center gap-2 bg-card px-4 font-bold text-foreground outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-card focus-visible:ring-offset-2 focus-visible:ring-offset-lapis"
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
    <Alert data-slot="unreachable-notice" className="reveal max-w-3xl gap-3 rounded-none border-0 border-t-4 border-foreground bg-muted p-4 text-foreground sm:p-6">
      <Unplug aria-hidden className="size-6!" />
      <AlertTitle>
        <h1 className="text-title sm:text-display">Cannot reach your deployment</h1>
      </AlertTitle>
      <AlertDescription className="grid max-w-prose gap-2 text-base text-foreground">
        <p>
          It last answered at {clock(ws.health.deployment.as_of)}. No agent data is shown, and none is kept on this device, so nothing here can be out of date.
        </p>
        <p>
          Agents keep running on the deployment, and protective orders rest at the broker. The Stop control still opens; from here it cannot deliver a request, so it tells
          you how to reach the broker directly.
        </p>
      </AlertDescription>
    </Alert>
  );
}

export function ScreenSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <div className="grid gap-(--section-gap)" aria-busy="true" aria-label="Loading" data-slot="skeleton">
      <div className="grid grid-cols-1 gap-(--seam) lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <Skeleton className="h-52 bg-lapis-soft" />
        <Skeleton className="h-52" />
      </div>
      <div className="grid gap-(--seam)">
        {Array.from({ length: rows }, (_, i) => (
          <div key={i} className="grid gap-(--seam) md:grid-cols-[8.5rem_minmax(0,1fr)_minmax(0,1.1fr)]">
            <Skeleton className="h-10 md:h-44" />
            <Skeleton className="h-44" />
            <Skeleton className="h-44 bg-marigold-soft" />
          </div>
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
