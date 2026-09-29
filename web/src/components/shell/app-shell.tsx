"use client";

import { type CSSProperties, type ReactNode, useMemo } from "react";
import { usePathname } from "next/navigation";
import { Sidebar } from "@cloudflare/kumo/components/sidebar";
import { buildMarket } from "@/fixtures/market";
import { canOpen } from "@/lib/access";
import { isRecordRoute } from "@/lib/frozen";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { allFeedsOk, quotes } from "@/lib/quotes";
import { can, useRole } from "@/lib/roles";
import { AccessDenied } from "./access-denied";
import { AccountBanners } from "./account-banners";
import { AppHeader } from "./app-header";
import { AppSidebar } from "./app-sidebar";
import { Dock } from "./dock";
import { TabNav } from "./nav";
import { StatusStrip } from "./status-strip";
import { Ticker } from "./ticker";

/** The nav width, handed to Kumo's Sidebar, which otherwise sets its own. */
const SIDEBAR_STYLE = { "--sidebar-width": "var(--nav-width)" } as CSSProperties;

/** Audit and admin read like a console; everything an owner lives in stays calm (DEC-204). */
export function densityFor(pathname: string): "calm" | "dense" {
  return /^\/(audit|settings|connections)(\/|$)/.test(pathname) ? "dense" : "calm";
}

export function AppShell({ children }: { children: ReactNode }) {
  const { ws, now } = useRuntime();
  const { role } = useRole();
  const pathname = usePathname();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;
  const seesAgents = can(role, "agents.view");
  // A record screen is frozen, and a stale or failing feed must be read in full: both keep the strip.
  const tape = useMemo(() => (seesAgents && allFeedsOk(ws) && !isRecordRoute(pathname) ? quotes(ws, buildMarket(ws)) : []), [ws, seesAgents, pathname]);

  return (
    <Sidebar.Provider collapsible="icon" mobileBreakpoint={1024} style={SIDEBAR_STYLE}>
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      <AppSidebar />
      <div className="flex min-h-dvh min-w-0 flex-1 flex-col bg-card">
        <AppHeader />
        {tape.length > 0 ? <Ticker ws={ws} now={now} quotes={tape} className="hidden sm:flex" /> : null}
        <div className={tape.length > 0 ? "sm:hidden" : undefined}>
          <StatusStrip ws={ws} now={now} className="px-(--page-x)" />
        </div>
        <AccountBanners />
        <main
          id="main"
          tabIndex={-1}
          data-density={densityFor(pathname)}
          className="mx-auto w-full max-w-(--content-max) flex-1 px-(--page-x) pt-(--page-top) pb-[calc(var(--tab-bar)+env(safe-area-inset-bottom)+2rem)] outline-none lg:pb-[calc(var(--dock-clearance)+2rem)]"
        >
          {canOpen(role, pathname) ? children : <AccessDenied role={role} />}
        </main>
      </div>
      <div className="fixed inset-x-0 bottom-0 z-30 lg:hidden">
        <TabNav approvals={open} />
      </div>
      <div className="fixed bottom-[calc(var(--dock-gap)+env(safe-area-inset-bottom))] left-1/2 z-30 hidden -translate-x-1/2 lg:block">
        <Dock approvals={open} />
      </div>
    </Sidebar.Provider>
  );
}
