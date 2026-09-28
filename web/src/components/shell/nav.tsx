"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";
import { Gauge, Inbox, ScrollText, Settings2, Workflow } from "lucide-react";

export const NAV = [
  { href: "/", label: "Dashboard", icon: Gauge },
  { href: "/approvals", label: "Approvals", icon: Inbox },
  { href: "/agents", label: "Agents", icon: Workflow },
  { href: "/audit", label: "Audit", icon: ScrollText },
  { href: "/settings", label: "Settings", icon: Settings2 },
] as const;

function isActive(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}

/** The approvals badge carries a count and nothing else. */
function Count({ n }: { n: number }) {
  if (n === 0) return null;
  return (
    <span data-slot="approvals-count" className="ml-auto inline-flex h-6 min-w-6 items-center justify-center bg-foreground px-1.5 font-mono text-caption font-bold text-background tabular">
      {n}
      <span className="sr-only"> open</span>
    </span>
  );
}

export function SideNav({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  return (
    <nav aria-label="Main" className="grid">
      {NAV.map(({ href, label, icon: Icon }) => {
        const active = isActive(pathname, href);
        return (
          <Link
            key={href}
            href={href}
            aria-current={active ? "page" : undefined}
            className={cn(
              "flex h-11 items-center gap-3 px-4 font-medium text-foreground transition-colors duration-(--duration-hover) hover:bg-muted",
              active && "bg-lapis font-bold text-lapis-foreground hover:bg-lapis",
            )}
          >
            <Icon className="size-4" aria-hidden />
            {label}
            {href === "/approvals" ? <Count n={approvals} /> : null}
          </Link>
        );
      })}
    </nav>
  );
}

export function TabNav({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  return (
    <nav aria-label="Main" className="grid grid-cols-5 border-t-2 border-foreground bg-card pb-[env(safe-area-inset-bottom)]">
      {NAV.map(({ href, label, icon: Icon }) => {
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
                <span data-slot="approvals-count" className="absolute -top-2 -right-3 inline-flex h-4.5 min-w-4.5 items-center justify-center bg-foreground px-1 font-mono text-label font-bold text-background tabular">
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
