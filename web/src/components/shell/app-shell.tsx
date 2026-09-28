"use client";

import type { CSSProperties, ReactNode } from "react";
import { usePathname } from "next/navigation";
import { Sidebar } from "@cloudflare/kumo/components/sidebar";
import { canOpen } from "@/lib/access";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { useRole } from "@/lib/roles";
import { AccessDenied } from "./access-denied";
import { AccountBanners } from "./account-banners";
import { AppHeader } from "./app-header";
import { AppSidebar } from "./app-sidebar";
import { TabNav } from "./nav";
import { StatusStrip } from "./status-strip";

/** Placard's nav width, handed to Kumo's Sidebar, which otherwise sets its own. */
const SIDEBAR_STYLE = { "--sidebar-width": "var(--nav-width)" } as CSSProperties;

export function AppShell({ children }: { children: ReactNode }) {
  const { ws, now } = useRuntime();
  const { role } = useRole();
  const pathname = usePathname();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;

  return (
    <Sidebar.Provider collapsible="icon" mobileBreakpoint={1024} style={SIDEBAR_STYLE}>
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      <AppSidebar />
      <div className="flex min-h-dvh min-w-0 flex-1 flex-col">
        <AppHeader />
        <StatusStrip ws={ws} now={now} className="border-b px-(--page-x)" />
        <AccountBanners />
        <main
          id="main"
          tabIndex={-1}
          className="mx-auto w-full max-w-(--content-max) flex-1 px-(--page-x) pt-(--page-top) pb-28 outline-none lg:pb-(--page-bottom)"
        >
          {canOpen(role, pathname) ? children : <AccessDenied role={role} />}
        </main>
      </div>
      <div className="fixed inset-x-0 bottom-0 z-30 lg:hidden">
        <TabNav approvals={open} />
      </div>
    </Sidebar.Provider>
  );
}
