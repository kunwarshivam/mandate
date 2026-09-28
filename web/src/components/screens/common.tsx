"use client";

import type { ReactNode } from "react";
import { cn } from "cn";
import { Unplug } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Skeleton } from "@/components/ui/skeleton";
import { clock } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";

export function PageHeader({ title, lead, children, className }: { title: string; lead?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <header className={cn("mb-6 flex flex-wrap items-end justify-between gap-4", className)}>
      <div className="grid gap-1.5">
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
    <section aria-labelledby={headingId} className={cn("grid content-start gap-3", className)}>
      <div className="flex items-baseline justify-between gap-3">
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
  return <div className={cn("rounded-xl border bg-card p-4 shadow-whisper sm:p-5", className)}>{children}</div>;
}

/** The deployment did not answer: no agent data is shown and nothing is kept on this device. */
export function UnreachableNotice() {
  const { ws } = useRuntime();
  return (
    <Alert data-slot="unreachable-notice" className="max-w-2xl gap-2 border-notice-border bg-notice p-5 text-foreground">
      <Unplug aria-hidden />
      <AlertTitle>
        <h1 className="text-heading">Cannot reach your deployment</h1>
      </AlertTitle>
      <AlertDescription className="grid gap-2 text-base text-foreground">
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
    <div className="grid gap-6" aria-busy="true" aria-label="Loading" data-slot="skeleton">
      <Skeleton className="h-10 w-56" />
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: rows }, (_, i) => (
          <Skeleton key={i} className="h-56 rounded-xl" />
        ))}
      </div>
      <Skeleton className="h-40 rounded-xl" />
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
