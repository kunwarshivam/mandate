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
  /** Beside the title, after the environment badge, such as an agent's mode on a phone. */
  status?: ReactNode;
  /** Actions on the right of the title row, such as Stop scoped to this agent. */
  actions?: ReactNode;
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

export function PageHeader({ title, description, environment, tabs, tabsLabel = "Sections", status, actions, children, className, tabsClassName }: PageHeaderProps) {
  const pathname = usePathname();
  const underline = `page-tabs-${tabsLabel}`;
  return (
    <header data-slot="page-header" className={cn("mb-(--block-gap) grid grid-cols-1 gap-(--block-gap)", className)}>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="grid min-w-0 gap-1">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <h1 className="text-h1">{title}</h1>
            {environment ? <EnvironmentBadge environment={environment} /> : null}
            {status}
          </div>
          {description ? <p className="max-w-measure text-muted-foreground">{description}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
      {children}
      {tabs && tabs.length > 0 ? (
        <nav aria-label={tabsLabel} className={cn("-mx-(--page-x) overflow-x-auto px-(--page-x) [scrollbar-width:none] lg:mx-0 lg:px-0", tabsClassName)}>
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
      ) : null}
    </header>
  );
}
