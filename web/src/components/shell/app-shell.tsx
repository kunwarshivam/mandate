"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { Lockup, Mark } from "./brand";
import { EnvironmentBadge } from "./environment-badge";
import { SideNav, TabNav } from "./nav";
import { StatusStrip } from "./status-strip";
import { StopControl } from "./stop-control";
import { ThemeToggle } from "./theme-toggle";

export function AppShell({ children }: { children: ReactNode }) {
  const { ws, now } = useRuntime();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;

  return (
    <div className="min-h-dvh lg:grid lg:grid-cols-[15rem_minmax(0,1fr)]">
      <a href="#main" className="sr-only z-50 rounded-md bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      <aside className="sticky top-0 hidden h-dvh flex-col gap-8 border-r px-3 py-5 lg:flex">
        <Link href="/" className="px-3 text-foreground" aria-label="Mandate, dashboard">
          <Lockup className="h-8 w-auto" />
        </Link>
        <SideNav approvals={open} />
        <div className="mt-auto grid gap-1 px-3 text-caption text-muted-foreground">
          <span>{ws.connection.broker}</span>
          <span className="font-mono text-[0.6875rem]">{ws.connection.connection_id}</span>
        </div>
      </aside>
      <div className="flex min-w-0 flex-col">
        <header className="sticky top-0 z-30 border-b bg-background/85 backdrop-blur-md">
          <div className="flex h-14 items-center gap-2 px-3 sm:gap-3 sm:px-4 lg:px-8">
            <Link href="/" className="text-foreground lg:hidden" aria-label="Mandate, dashboard">
              <Mark className="size-7" />
            </Link>
            <EnvironmentBadge environment={ws.environment} />
            <div className="ml-auto flex items-center gap-1 sm:gap-2">
              <ThemeToggle />
              <StopControl />
            </div>
          </div>
          <StatusStrip ws={ws} now={now} className="border-t lg:px-8" />
        </header>
        <main id="main" tabIndex={-1} className="mx-auto w-full max-w-6xl flex-1 px-4 pt-6 pb-28 outline-none sm:px-6 lg:px-8 lg:pb-16">
          {children}
        </main>
      </div>
      <div className="fixed inset-x-0 bottom-0 z-30 lg:hidden">
        <TabNav approvals={open} />
      </div>
    </div>
  );
}
