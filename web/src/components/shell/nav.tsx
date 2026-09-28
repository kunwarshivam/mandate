"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "cn";
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
    <span data-slot="approvals-count" className="ml-auto inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-primary px-1.5 font-mono text-[0.6875rem] font-semibold text-primary-foreground tabular">
      {n}
      <span className="sr-only"> open</span>
    </span>
  );
}

export function SideNav({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  return (
    <nav aria-label="Main" className="grid gap-0.5">
      {NAV.map(({ href, label, icon: Icon }) => {
        const active = isActive(pathname, href);
        return (
          <Link
            key={href}
            href={href}
            aria-current={active ? "page" : undefined}
            className={cn(
              "press flex h-10 items-center gap-3 rounded-lg px-3 text-sm font-medium text-muted-foreground hover:bg-muted hover:text-foreground",
              active && "bg-muted text-foreground",
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
    <nav aria-label="Main" className="grid grid-cols-5 border-t bg-card/95 pb-[env(safe-area-inset-bottom)] backdrop-blur">
      {NAV.map(({ href, label, icon: Icon }) => {
        const active = isActive(pathname, href);
        return (
          <Link
            key={href}
            href={href}
            aria-current={active ? "page" : undefined}
            className={cn("press relative grid h-14 place-items-center content-center gap-0.5 text-[0.6875rem] font-medium text-muted-foreground", active && "text-foreground")}
          >
            <span className="relative">
              <Icon className="size-5" aria-hidden />
              {href === "/approvals" && approvals > 0 ? (
                <span data-slot="approvals-count" className="absolute -top-1.5 -right-2.5 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 font-mono text-[0.625rem] font-semibold text-primary-foreground tabular">
                  {approvals}
                  <span className="sr-only"> open</span>
                </span>
              ) : null}
            </span>
            {label}
            {active ? <span className="absolute inset-x-5 top-0 h-0.5 rounded-full bg-primary" aria-hidden /> : null}
          </Link>
        );
      })}
    </nav>
  );
}
