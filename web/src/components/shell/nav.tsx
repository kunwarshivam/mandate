"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { motion } from "motion/react";
import { GearSix, House, type Icon as PhosphorIcon, Robot, Scroll, Tray } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { type Capability, useRole, can } from "@/lib/roles";

const TABS: ReadonlyArray<{ href: string; label: string; icon: PhosphorIcon; needs: Capability }> = [
  { href: "/", label: "Home", icon: House, needs: "agents.view" },
  { href: "/approvals", label: "Approvals", icon: Tray, needs: "agents.view" },
  { href: "/agents", label: "Agents", icon: Robot, needs: "agents.view" },
  { href: "/audit", label: "Audit", icon: Scroll, needs: "audit.view" },
  { href: "/settings", label: "Workspace", icon: GearSix, needs: "workspace.view" },
];

function isActive(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}

/**
 * Phones keep a bottom tab bar; the sidebar opens as a sheet from the header. The current tab's
 * icon fills and sits on a pale ultramarine pill that glides between tabs; with reduced motion it jumps.
 */
export function TabNav({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  const { role } = useRole();
  const tabs = TABS.filter((t) => can(role, t.needs));
  return (
    <nav
      aria-label="Main"
      className="grid border-t border-border/70 bg-card pb-[env(safe-area-inset-bottom)]"
      style={{ gridTemplateColumns: `repeat(${tabs.length}, minmax(0, 1fr))` }}
    >
      {tabs.map(({ href, label, icon: Icon }) => {
        const active = isActive(pathname, href);
        return (
          <Link
            key={href}
            href={href}
            aria-current={active ? "page" : undefined}
            className={cn(
              "press relative grid h-(--tab-bar) min-w-0 place-items-center content-center gap-1 px-0.5 text-[0.6875rem] leading-tight font-medium text-muted-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset",
              active && "text-lapis",
            )}
          >
            <span className="relative grid h-7 w-14 place-items-center">
              {active ? (
                <motion.span
                  layoutId="tab-pill"
                  className="absolute inset-0 rounded-full bg-lapis-soft ring-1 ring-inset ring-lapis-line"
                  transition={{ type: "spring", duration: 0.3, bounce: 0.1 }}
                  aria-hidden
                />
              ) : null}
              <Icon className="relative size-5.5" weight={active ? "fill" : "regular"} aria-hidden />
              {href === "/approvals" && approvals > 0 ? (
                <span
                  data-slot="approvals-count"
                  className="absolute -top-1 right-1 inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-full bg-lapis px-1 font-mono text-[0.6875rem] font-semibold text-lapis-foreground tabular ring-2 ring-card"
                >
                  {approvals}
                  <span className="sr-only"> open</span>
                </span>
              ) : null}
            </span>
            <span className="max-w-full truncate">{label}</span>
          </Link>
        );
      })}
    </nav>
  );
}
