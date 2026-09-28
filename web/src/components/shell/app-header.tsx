"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Breadcrumbs } from "@cloudflare/kumo/components/breadcrumbs";
import { DropdownMenu } from "@cloudflare/kumo/components/dropdown";
import { Sidebar } from "@cloudflare/kumo/components/sidebar";
import { Bell, CaretUpDown, Tray, UserCircle } from "@phosphor-icons/react";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { can, useRole } from "@/lib/roles";
import { crumbsFor } from "@/lib/screens";
import { ApprovalsCount } from "./app-sidebar";
import { Wordmark } from "./brand";
import { CommandMenu } from "./command-menu";
import { EnvironmentBadge } from "./environment-badge";
import { StopControl } from "./stop-control";

/** Fixture workspaces: the switcher shows the shape of the control, and only one is connected. */
const WORKSPACES = [
  { id: "ws_paper", label: "Paper workspace", current: true },
  { id: "ws_second", label: "Second workspace", current: false },
] as const;

const ICON_LINK =
  "press relative inline-flex size-11 shrink-0 items-center justify-center text-foreground outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring";

function WorkspaceSwitcher() {
  const current = WORKSPACES.find((w) => w.current) ?? WORKSPACES[0];
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger
        render={<button type="button" />}
        className="press inline-flex h-11 max-w-48 shrink-0 items-center gap-1.5 px-2 text-sm font-bold outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring"
      >
        <span className="truncate">{current.label}</span>
        <CaretUpDown className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="start">
        <DropdownMenu.Label>Workspaces (fixture)</DropdownMenu.Label>
        {WORKSPACES.map((w) => (
          <DropdownMenu.Item key={w.id} selected={w.current} disabled={!w.current}>
            {w.label}
            {w.current ? null : <span className="ml-2 text-caption text-muted-foreground">not connected in this preview</span>}
          </DropdownMenu.Item>
        ))}
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}

function UserMenu() {
  const { role } = useRole();
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger render={<button type="button" />} aria-label="Your account" className={ICON_LINK}>
        <UserCircle className="size-5" aria-hidden />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        <DropdownMenu.Label>You, {role}</DropdownMenu.Label>
        {can(role, "workspace.view") ? (
          <>
            <DropdownMenu.LinkItem render={<Link href="/settings/profile" />}>Profile</DropdownMenu.LinkItem>
            <DropdownMenu.LinkItem render={<Link href="/settings/notifications" />}>Notifications</DropdownMenu.LinkItem>
          </>
        ) : (
          <DropdownMenu.LinkItem render={<Link href="/audit" />}>Audit</DropdownMenu.LinkItem>
        )}
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}

/**
 * The header always renders, whatever is loading: the Stop control, the paper badge, and the way
 * home never wait for workspace data (brief §5, rule 13).
 */
export function AppHeader() {
  const pathname = usePathname();
  const { ws, now } = useRuntime();
  const { role } = useRole();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;
  // Deep pages keep their last three crumbs; the page's tabs and the sidebar carry the rest of the way back.
  const crumbs = crumbsFor(pathname, (id) => ws.agents.find((a) => a.agent_id === id)?.label).slice(-3);
  const seesAgents = can(role, "agents.view");

  return (
    <header className="sticky top-0 z-30 border-b bg-background">
      <div className="flex h-14 items-center gap-1 px-(--page-x) sm:gap-2">
        <Sidebar.Trigger className="lg:hidden" />
        <Link href="/" className="shrink-0 px-1 text-foreground" aria-label="Owlhead, dashboard">
          <Wordmark className="text-xl sm:text-2xl" />
        </Link>
        <div className="hidden lg:block">
          <WorkspaceSwitcher />
        </div>
        <div className="hidden min-w-0 flex-1 md:flex">
          <Breadcrumbs size="sm" className="mr-0 [&_a]:min-w-0 [&_a]:shrink-[4] [&_a>span]:truncate">
            {crumbs.flatMap((c, i) => [
              ...(i > 0 ? [<Breadcrumbs.Separator key={`sep-${c.href}`} />] : []),
              i === crumbs.length - 1 ? (
                <Breadcrumbs.Current key={c.href}>{c.label}</Breadcrumbs.Current>
              ) : (
                <Breadcrumbs.Link key={c.href} href={c.href}>
                  {c.label}
                </Breadcrumbs.Link>
              ),
            ])}
          </Breadcrumbs>
        </div>
        <div className="ml-auto flex items-center gap-1 sm:gap-2">
          <CommandMenu />
          <EnvironmentBadge environment={ws.environment} />
          {seesAgents ? (
            <>
              <Link href="/approvals" aria-label="Approvals" className={`${ICON_LINK} max-sm:hidden`}>
                <Tray className="size-5" aria-hidden />
                <ApprovalsCount n={open} className="absolute top-1 right-0.5" />
              </Link>
              <Link href="/alerts" aria-label="Alerts" className={`${ICON_LINK} max-sm:hidden`}>
                <Bell className="size-5" aria-hidden />
              </Link>
            </>
          ) : null}
          <div className="max-sm:hidden">
            <UserMenu />
          </div>
          <StopControl />
        </div>
      </div>
    </header>
  );
}
