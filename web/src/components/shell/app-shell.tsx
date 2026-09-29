"use client";

import { type ReactNode, useMemo } from "react";
import { usePathname } from "next/navigation";
import { canOpen } from "@/lib/access";
import { isRecordRoute } from "@/lib/frozen";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { allFeedsOk } from "@/lib/feeds";
import { can, useRole } from "@/lib/roles";
import { wireItems } from "@/lib/wire";
import { FixtureTag } from "@/components/domain/placeholders";
import { AccessDenied } from "./access-denied";
import { AccountBanners } from "./account-banners";
import { AppHeader } from "./app-header";
import { Dock } from "./dock";
import { TabNav } from "./nav";
import { StatusStrip } from "./status-strip";
import { Wire } from "./wire";

/** Audit and admin read like a console; everything an owner lives in stays calm (DEC-204). */
export function densityFor(pathname: string): "calm" | "dense" {
  return /^\/(audit|settings|connections)(\/|$)/.test(pathname) ? "dense" : "calm";
}

/**
 * The frame. From `lg` up: the header, then the agent wire while every feed answers or the status
 * strip, and the dock. Below `lg` (DEC-207): the header, a feed banner only while a feed is stale or
 * down, and the tab bar; a record screen keeps the full strip, because a decision is read against
 * every feed's age, not against the absence of a warning.
 */
export function AppShell({ children }: { children: ReactNode }) {
  const { ws, now } = useRuntime();
  const { role } = useRole();
  const pathname = usePathname();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;
  const seesAgents = can(role, "agents.view");
  const record = isRecordRoute(pathname);
  const fresh = allFeedsOk(ws);
  // A record screen is frozen, and a stale or failing feed must be read in full: both keep the strip.
  const wire = seesAgents && ws.agents.length > 0 && fresh && !record;
  const banner = !record && !fresh && ws.status !== "loading";
  const items = useMemo(() => (wire ? wireItems(ws, now, role) : []), [wire, ws, now, role]);

  return (
    <div className="relative isolate flex min-h-svh w-full">
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      <div className="flex min-h-dvh min-w-0 flex-1 flex-col bg-card">
        <AppHeader />
        {wire ? (
          <Wire ws={ws} now={now} items={items} className="hidden lg:flex" />
        ) : (
          <div className={record ? undefined : "max-lg:hidden"}>
            <StatusStrip ws={ws} now={now} className="px-(--page-x)" />
          </div>
        )}
        {banner ? (
          <div className="lg:hidden">
            <StatusStrip ws={ws} now={now} variant="banner" className="px-(--page-x)" />
          </div>
        ) : null}
        <AccountBanners />
        <main
          id="main"
          tabIndex={-1}
          data-density={densityFor(pathname)}
          className="mx-auto w-full max-w-(--content-max) flex-1 px-(--page-x) pt-(--page-top) pb-10 outline-none lg:pb-[calc(var(--dock-clearance)+2rem)]"
        >
          {canOpen(role, pathname) ? children : <AccessDenied role={role} />}
        </main>
        <div
          data-slot="phone-footer"
          className="mx-auto w-full max-w-(--content-max) px-(--page-x) pb-[calc(var(--tab-bar)+env(safe-area-inset-bottom)+2rem)] lg:hidden"
        >
          {record ? null : <FixtureTag />}
        </div>
      </div>
      <div className="fixed inset-x-0 bottom-0 z-30 lg:hidden">
        <TabNav approvals={open} />
      </div>
      <div className="fixed bottom-[calc(var(--dock-gap)+env(safe-area-inset-bottom))] left-1/2 z-30 hidden -translate-x-1/2 lg:block">
        <Dock approvals={open} />
      </div>
    </div>
  );
}
