"use client";

import { type ReactNode, useRef } from "react";
import { usePathname } from "next/navigation";
import { canOpen } from "@/lib/access";
import { isRecordRoute } from "@/lib/frozen";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { allFeedsOk } from "@/lib/feeds";
import { useRole } from "@/lib/roles";
import { cn } from "@/lib/utils";
import { FixtureTag, InlineDisclosures } from "@/components/domain/placeholders";
import { AccessDenied } from "./access-denied";
import { AccountBanners } from "./account-banners";
import { AppHeader } from "./app-header";
import { Dock } from "./dock";
import { denseEdgeClass } from "./dense-edge-class";
import { PAGE_FRAME, fullBleed } from "./frame";
import { TabNav } from "./nav";
import { StatusStrip } from "./status-strip";
import { StopSheetHost } from "./stop-control";

/** A connection's admin screens read like a console; everything an owner lives in stays calm (DEC-204). */
export function densityFor(pathname: string): "calm" | "dense" {
  return /^\/connections\//.test(pathname) ? "dense" : "calm";
}

/**
 * The dense screens' column: the frame's own pieces with the centring gone, so the column keeps
 * the calm page's left edge (`denseEdgeClass()`, critique C-17) and widens only to the right,
 * never past the density's own 90rem bound. Built by hand, not by appending to `PAGE_FRAME`: the
 * merge helper keeps `mx-auto` beside a later `ml-`, so the centring must be replaced, not joined.
 */
const DENSE_FRAME = cn(
  denseEdgeClass(),
  "max-w-(--content-max) px-(--page-x) pt-(--page-top) pb-10",
);

/**
 * The frame: the header, then nothing while every feed answers (DEC-215). From `lg` up, the status
 * strip while a feed is stale or down, and the dock; below `lg` (DEC-207), a one-line banner of the
 * failing feeds, and the tab bar. A record screen keeps the full strip at every width, because a
 * decision is read against every feed's age, not against the absence of a warning. With no strip,
 * the "Fixture data" tag sits at the foot of the page. The More sheet opens into a layer inside the
 * frame, under the header and the tab bar, so it rises from behind the bar and never covers Stop.
 * A thread, an agent's or a new agent's setup, fills the window edge to edge, with no gutter and
 * no page footer (DEC-481, DEC-2293).
 */
export function AppShell({ children }: { children: ReactNode }) {
  return <Frame>{children}</Frame>;
}

function Frame({ children }: { children: ReactNode }) {
  const { ws, now } = useRuntime();
  const { role } = useRole();
  const pathname = usePathname();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;
  const record = isRecordRoute(pathname);
  const fresh = allFeedsOk(ws);
  const strip = record || !fresh;
  const banner = !record && !fresh && ws.status !== "loading";
  const sheetLayer = useRef<HTMLDivElement>(null);
  const bleed = fullBleed(pathname);
  const density = densityFor(pathname);

  return (
    <div className="group/frame relative isolate flex min-h-svh w-full">
      <a href="#main" className="sr-only z-50 bg-card px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
        Skip to content
      </a>
      <div className={cn("flex min-h-dvh min-w-0 flex-1 flex-col bg-card", bleed && "h-dvh max-lg:pb-[calc(var(--tab-bar)+env(safe-area-inset-bottom))]")}>
        <AppHeader className="max-lg:group-has-[[data-slot=thread-pane]]/frame:hidden" sheetLayer={sheetLayer} />
        {strip ? (
          <div className={record ? undefined : "max-lg:hidden"}>
            <StatusStrip ws={ws} now={now} className="px-(--page-x)" />
          </div>
        ) : null}
        {banner ? (
          <div className="lg:hidden">
            <StatusStrip ws={ws} now={now} variant="banner" className="px-(--page-x)" />
          </div>
        ) : null}
        <AccountBanners />
        <main
          id="main"
          tabIndex={-1}
          data-density={density}
          className={cn(
            "flex-1 outline-none",
            bleed ? "flex min-h-0 w-full flex-col" : density === "dense" ? DENSE_FRAME : PAGE_FRAME,
            strip && !bleed && "lg:pb-[calc(var(--dock-clearance)+2rem)]",
          )}
        >
          <InlineDisclosures inline={isRecordRoute(pathname)}>{canOpen(role, pathname) ? children : <AccessDenied role={role} />}</InlineDisclosures>
        </main>
        {bleed ? null : (
          <div
            data-slot="page-footer"
            className={cn(
              "mx-auto w-full max-w-(--content-max) px-(--page-x) pb-[calc(var(--tab-bar)+env(safe-area-inset-bottom)+2rem)]",
              strip ? "lg:hidden" : "lg:pb-[calc(var(--dock-clearance)+2rem)]",
            )}
          >
            {record ? null : <FixtureTag scope="workspace" />}
          </div>
        )}
      </div>
      <div ref={sheetLayer} data-slot="sheet-layer" />
      <div className="fixed inset-x-0 bottom-0 z-30 lg:hidden">
        <TabNav approvals={open} sheetLayer={sheetLayer} />
      </div>
      <div className="fixed bottom-[calc(var(--dock-gap)+env(safe-area-inset-bottom))] left-1/2 z-30 hidden -translate-x-1/2 lg:block">
        <Dock approvals={open} />
      </div>
      <StopSheetHost />
    </div>
  );
}
