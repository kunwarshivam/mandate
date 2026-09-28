"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import type { Environment } from "@/fixtures/types";

/**
 * Kumo's PageHeader block (`kumo add PageHeader`), in Placard. Route tabs are links in a `nav`, not a
 * tablist: each is a page with its own address, so nothing required hides behind a tab. The
 * environment slot is filled on every record screen, so the paper badge sits beside the title.
 */
export interface PageTab {
  href: string;
  label: string;
}

export interface PageHeaderProps {
  title: string;
  description?: ReactNode;
  /** Record screens pass the workspace environment; the badge sits beside the title. */
  environment?: Environment;
  tabs?: readonly PageTab[];
  tabsLabel?: string;
  /** Actions on the right of the title row, such as Stop scoped to this agent. */
  actions?: ReactNode;
  /** Extra content under the title, such as a mode field. */
  children?: ReactNode;
  className?: string;
}

function isCurrent(pathname: string, href: string, tabs: readonly PageTab[]): boolean {
  if (pathname === href) return true;
  const deeper = tabs.filter((t) => pathname.startsWith(`${t.href}/`));
  const longest = deeper.sort((a, b) => b.href.length - a.href.length)[0];
  return longest?.href === href;
}

export function PageHeader({ title, description, environment, tabs, tabsLabel = "Sections", actions, children, className }: PageHeaderProps) {
  const pathname = usePathname();
  return (
    <header data-slot="page-header" className={cn("mb-(--block-gap) grid grid-cols-1 gap-(--block-gap)", className)}>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="grid min-w-0 gap-1">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <h1 className="text-h1 sm:text-h1">{title}</h1>
            {environment ? <EnvironmentBadge environment={environment} /> : null}
          </div>
          {description ? <p className="max-w-prose text-muted-foreground">{description}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
      {children}
      {tabs && tabs.length > 0 ? (
        <nav aria-label={tabsLabel} className="-mx-(--page-x) overflow-x-auto border-b border-foreground px-(--page-x) lg:mx-0 lg:px-0">
          <ul className="flex min-w-max gap-1">
            {tabs.map((tab) => {
              const current = isCurrent(pathname, tab.href, tabs);
              return (
                <li key={tab.href}>
                  <Link
                    href={tab.href}
                    aria-current={current ? "page" : undefined}
                    className={cn(
                      "relative inline-flex h-11 items-center px-3 text-sm font-medium text-muted-foreground outline-none transition-colors duration-(--duration-hover) hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset",
                      current && "font-semibold text-foreground after:absolute after:inset-x-0 after:-bottom-0.5 after:h-1 after:bg-lapis",
                    )}
                  >
                    {tab.label}
                  </Link>
                </li>
              );
            })}
          </ul>
        </nav>
      ) : null}
    </header>
  );
}
