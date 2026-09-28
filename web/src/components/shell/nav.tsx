"use client";

import type { ComponentType } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { GearSix, House, Robot, Scroll, Tray } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { type Capability, useRole, can } from "@/lib/roles";

const TABS: ReadonlyArray<{ href: string; label: string; icon: ComponentType<{ className?: string }>; needs: Capability }> = [
  { href: "/", label: "Home", icon: House, needs: "agents.view" },
  { href: "/approvals", label: "Approvals", icon: Tray, needs: "agents.view" },
  { href: "/agents", label: "Agents", icon: Robot, needs: "agents.view" },
  { href: "/audit", label: "Audit", icon: Scroll, needs: "audit.view" },
  { href: "/settings", label: "Workspace", icon: GearSix, needs: "workspace.view" },
];

function isActive(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}

/** Phones keep a bottom tab bar; the sidebar opens as a sheet from the header. */
export function TabNav({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  const { role } = useRole();
  const tabs = TABS.filter((t) => can(role, t.needs));
  return (
    <nav
      aria-label="Main"
      className="grid border-t-2 border-foreground bg-card pb-[env(safe-area-inset-bottom)]"
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
              "press relative grid h-14 min-w-0 place-items-center content-center gap-0.5 px-0.5 text-[0.6875rem] leading-tight font-medium text-muted-foreground",
              active && "font-bold text-foreground",
            )}
          >
            <span className="relative">
              <Icon className="size-5" aria-hidden />
              {href === "/approvals" && approvals > 0 ? (
                <span
                  data-slot="approvals-count"
                  className="absolute -top-2 -right-3 inline-flex h-4.5 min-w-4.5 items-center justify-center bg-foreground px-1 font-mono text-label font-bold text-background tabular"
                >
                  {approvals}
                  <span className="sr-only"> open</span>
                </span>
              ) : null}
            </span>
            <span className="max-w-full truncate">{label}</span>
            {active ? <span className="absolute inset-x-3 top-0 h-1 bg-lapis" aria-hidden /> : null}
          </Link>
        );
      })}
    </nav>
  );
}
