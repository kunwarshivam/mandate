"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { motion } from "motion/react";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import type { Environment } from "@/fixtures/types";

/**
 * Kumo's PageHeader block (`kumo add PageHeader`), calm. Route tabs are links in a `nav`, not a
 * tablist: each is a page with its own address, so nothing required hides behind a tab. The current
 * tab's underline slides from the tab you left (a shared layout, so it jumps with reduced motion). The
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
  /** Beside the title, after the environment badge, such as an agent's mode. */
  status?: ReactNode;
  /** Before the title, such as an agent's owl. */
  icon?: ReactNode;
  /** Actions on the right of the title row, such as Stop scoped to this agent. */
  actions?: ReactNode;
  /** The actions are icon-sized on a phone, so they keep the title's row there instead of wrapping under it (DEC-482). */
  actionsInline?: boolean;
  /** Extra content under the title, such as a mode field. */
  children?: ReactNode;
  className?: string;
  tabsClassName?: string;
}

/** One tab is current: the exact match, else the deepest tab the path sits under. */
export function isCurrentTab(pathname: string, href: string, tabs: readonly PageTab[]): boolean {
  if (tabs.some((t) => t.href === pathname)) return pathname === href;
  const deeper = tabs.filter((t) => pathname.startsWith(`${t.href}/`));
  const longest = deeper.sort((a, b) => b.href.length - a.href.length)[0];
  return longest?.href === href;
}

/** The route tabs alone, for a pane inside a page, such as a thread's Chat and Desk. */
export function PageTabs({ tabs, label, className }: { tabs: readonly PageTab[]; label: string; className?: string }) {
  const pathname = usePathname();
  const underline = `page-tabs-${label}`;
  return (
    <nav aria-label={label} className={cn("-mx-(--page-x) overflow-x-auto px-(--page-x) [scrollbar-width:none] lg:mx-0 lg:px-0", className)}>
      <ul className="flex min-w-max gap-1 border-b border-border/70">
        {tabs.map((tab) => {
          const current = isCurrentTab(pathname, tab.href, tabs);
          return (
            <li key={tab.href} className="group/tab">
              <Link
                href={tab.href}
                aria-current={current ? "page" : undefined}
                className={cn(
                  "relative inline-flex h-11 items-center px-3 text-sm text-muted-foreground outline-none transition-colors duration-(--duration-hover) group-first/tab:pl-0 hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset",
                  current && "font-medium text-foreground",
                )}
              >
                {tab.label}
                {current ? (
                  <motion.span
                    layoutId={underline}
                    aria-hidden
                    data-slot="tab-underline"
                    className="absolute inset-x-3 -bottom-px h-0.5 rounded-xs bg-lapis-line group-first/tab:left-0"
                    transition={{ type: "spring", duration: 0.35, bounce: 0.2 }}
                  />
                ) : null}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

/**
 * Below `lg` the app header already carries the paper badge, so the title's copy of it shows from
 * `lg` only, and the title row keeps its height for what the page is about (DEC-482).
 */
export function PageHeader({ title, description, environment, tabs, tabsLabel = "Sections", status, icon, actions, actionsInline = false, children, className, tabsClassName }: PageHeaderProps) {
  return (
    <header data-slot="page-header" className={cn("mb-(--block-gap) grid grid-cols-1 gap-(--block-gap)", className)}>
      <div className={cn("flex flex-wrap items-end justify-between gap-3", actionsInline && "max-lg:flex-nowrap max-lg:items-center")}>
        <div className="flex min-w-0 items-center gap-4 max-lg:gap-3">
          {icon}
          <div className="grid min-w-0 gap-1">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <h1 className="text-h1">{title}</h1>
              {environment ? <EnvironmentBadge environment={environment} className="max-lg:hidden" /> : null}
              {status}
            </div>
            {description ? <p className="max-w-measure text-muted-foreground">{description}</p> : null}
          </div>
        </div>
        {actions ? <div className={cn("flex flex-wrap items-center gap-2", actionsInline && "max-lg:shrink-0")}>{actions}</div> : null}
      </div>
      {children}
      {tabs && tabs.length > 0 ? <PageTabs tabs={tabs} label={tabsLabel} className={tabsClassName} /> : null}
    </header>
  );
}
