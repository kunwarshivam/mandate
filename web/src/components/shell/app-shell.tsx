"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { Wordmark } from "./brand";
import { EnvironmentBadge } from "./environment-badge";
import { SideNav, TabNav } from "./nav";
import { StatusStrip } from "./status-strip";
import { StopControl } from "./stop-control";

export function AppShell({ children }: { children: ReactNode }) {
  const { ws, now } = useRuntime();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;

  return (
    <div className="min-h-dvh lg:grid lg:grid-cols-[var(--sidebar-width)_minmax(0,1fr)]">
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      <aside className="sticky top-0 hidden h-dvh flex-col gap-6 border-r bg-background py-4 lg:flex">
        <Link href="/" className="px-4 text-foreground" aria-label="Owlhead, dashboard">
          <Wordmark className="text-[2rem]" />
        </Link>
        <SideNav approvals={open} />
        <div className="mt-auto grid gap-0.5 bg-lapis px-4 py-3 text-lapis-foreground" data-slot="account">
          <span className="label-caps text-lapis-muted">Account</span>
          <span className="text-sm font-bold">{ws.connection.broker}</span>
          <span className="font-mono text-caption break-all text-lapis-muted" translate="no">
            {ws.connection.connection_id}
          </span>
        </div>
      </aside>
      <div className="flex min-w-0 flex-col">
        <header className="sticky top-0 z-30 border-b bg-background">
          <div className="flex h-14 items-center gap-2 px-(--page-x) sm:gap-3">
            <Link href="/" className="text-foreground lg:hidden" aria-label="Owlhead, dashboard">
              <Wordmark className="text-2xl" />
            </Link>
            <EnvironmentBadge environment={ws.environment} />
            <div className="ml-auto flex items-center">
              <StopControl />
            </div>
          </div>
          <StatusStrip ws={ws} now={now} className="border-t px-(--page-x)" />
        </header>
        <main
          id="main"
          tabIndex={-1}
          className="mx-auto w-full max-w-(--content-max) flex-1 px-(--page-x) pt-(--page-top) pb-28 outline-none lg:pb-(--page-bottom)"
        >
          {children}
        </main>
      </div>
      <div className="fixed inset-x-0 bottom-0 z-30 lg:hidden">
        <TabNav approvals={open} />
      </div>
    </div>
  );
}
