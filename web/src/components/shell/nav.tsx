"use client";

import { type ReactNode, type RefObject, useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { motion } from "motion/react";
import { MoreHorizontal } from "pixelarticons/react/MoreHorizontal.js";
import { cn } from "@/lib/utils";
import { can, useRole } from "@/lib/roles";
import { isCurrent } from "./dock";
import { MoreSheet, moreGroups, phoneTabs } from "./more-sheet";
import { StopButton } from "./stop-control";

const TAB =
  "press relative grid h-(--tab-bar) min-w-0 place-items-center content-center gap-1 px-0.5 text-[0.6875rem] leading-tight font-medium text-muted-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset";

function TabFace({ active, icon, label, badge }: { active: boolean; icon: ReactNode; label: string; badge?: ReactNode }) {
  return (
    <>
      <span className="relative grid h-7 w-14 place-items-center">
        {active ? (
          <motion.span
            layoutId="tab-pill"
            className="absolute inset-0 rounded-lg bg-lapis-soft ring-1 ring-inset ring-lapis-line"
            transition={{ type: "spring", duration: 0.3, bounce: 0.1 }}
            aria-hidden
          />
        ) : null}
        {icon}
        {badge}
      </span>
      <span className="max-w-full truncate">{label}</span>
    </>
  );
}

/**
 * The phone's one navigation (DEC-207, DEC-479): Home, Messages with the count of requests waiting,
 * Agents, and More, a sheet with every other screen, Approvals among them, then Stop at the bar's
 * end for a role that may stop (DEC-452). A role
 * that sees no agents gets its home and More. The current tab's
 * icon sits on a pale pill that glides between tabs; with reduced motion it jumps.
 */
export function TabNav({ approvals, sheetLayer }: { approvals: number; sheetLayer?: RefObject<HTMLElement | null> }) {
  const pathname = usePathname();
  const { role } = useRole();
  const [more, setMore] = useState(false);
  const tabs = phoneTabs(role);
  const onTab = tabs.some((t) => isCurrent(pathname, t.href));
  const inMore = !onTab && moreGroups(role).some((g) => g.links.some((l) => isCurrent(pathname, l.href)));
  const moreActive = more || inMore;
  const stops = can(role, "stop.open");
  return (
    <nav
      aria-label="Main"
      className="glass grid border-t pb-[env(safe-area-inset-bottom)]"
      style={{ gridTemplateColumns: `repeat(${tabs.length + 1}, minmax(0, 1fr))${stops ? " auto" : ""}` }}
    >
      {tabs.map(({ href, label, icon: Glyph }) => {
        const active = !more && isCurrent(pathname, href);
        return (
          <Link key={href} href={href} aria-current={isCurrent(pathname, href) ? "page" : undefined} className={cn(TAB, active && "text-lapis")}>
            <TabFace
              active={active}
              label={label}
              icon={<Glyph className="relative size-6" aria-hidden />}
              badge={
                href === "/messages" && approvals > 0 ? (
                  <span
                    data-slot="approvals-count"
                    className="absolute -top-1 right-1 inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-sm bg-lapis px-1 font-mono text-[0.6875rem] font-semibold text-lapis-foreground tabular ring-2 ring-card"
                  >
                    {approvals}
                    <span className="sr-only"> open</span>
                  </span>
                ) : null
              }
            />
          </Link>
        );
      })}
      <button
        type="button"
        data-slot="more-tab"
        aria-haspopup="dialog"
        aria-expanded={more}
        aria-current={inMore ? "page" : undefined}
        onClick={() => setMore(true)}
        className={cn(TAB, moreActive && "text-lapis")}
      >
        <TabFace
          active={moreActive}
          label="More"
          icon={<MoreHorizontal className="relative size-6" aria-hidden />}
        />
      </button>
      {stops ? <StopButton place="tab" className="pr-3 pl-1" /> : null}
      <MoreSheet open={more} onOpenChange={setMore} container={sheetLayer} />
    </nav>
  );
}
