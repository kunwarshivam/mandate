"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Breadcrumbs } from "@cloudflare/kumo/components/breadcrumbs";
import { DropdownMenu } from "@cloudflare/kumo/components/dropdown";
import { Sidebar } from "@cloudflare/kumo/components/sidebar";
import { Bell, Briefcase, CaretUpDown, DotsThreeVertical, Tray, UserCircle } from "@phosphor-icons/react";
import { canOpen, homeFor } from "@/lib/access";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { type Role, can, useRole } from "@/lib/roles";
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
  "press relative inline-flex size-11 shrink-0 items-center justify-center rounded-full text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring";

/** Below `xl` the trail keeps its last two crumbs: Kumo renders the full trail as the nav's last child. */
const TWO_CRUMBS = "max-xl:[&>div:last-child>*:nth-child(-n+2)]:hidden";

function WorkspaceSwitcher() {
  const current = WORKSPACES.find((w) => w.current) ?? WORKSPACES[0];
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger
        render={<button type="button" />}
        aria-label={`Workspace: ${current.label}`}
        className="press inline-flex h-10 max-w-48 shrink-0 items-center gap-1.5 rounded-md px-2.5 text-sm font-medium outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring"
      >
        <Briefcase className="size-5 shrink-0 xl:hidden" aria-hidden />
        <span className="truncate max-xl:sr-only">{current.label}</span>
        <CaretUpDown className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="start">
        <DropdownMenu.Group>
          <DropdownMenu.Label>Workspaces (fixture)</DropdownMenu.Label>
          {WORKSPACES.map((w) => (
            <DropdownMenu.Item key={w.id} selected={w.current} disabled={!w.current}>
              {w.label}
              {w.current ? null : <span className="ml-2 text-caption text-muted-foreground">not connected in this preview</span>}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Group>
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}

/** Base UI's menu labels must sit inside a group. */
function AccountLinks({ role }: { role: Role }) {
  return (
    <DropdownMenu.Group>
      <DropdownMenu.Label>You, {role}</DropdownMenu.Label>
      {can(role, "workspace.view") ? (
        <>
          <DropdownMenu.LinkItem render={<Link href="/settings/profile" />}>Profile</DropdownMenu.LinkItem>
          <DropdownMenu.LinkItem render={<Link href="/settings/notifications" />}>Notifications</DropdownMenu.LinkItem>
        </>
      ) : (
        <DropdownMenu.LinkItem render={<Link href="/audit" />}>Audit</DropdownMenu.LinkItem>
      )}
    </DropdownMenu.Group>
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
        <AccountLinks role={role} />
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}

/** Alerts and the account menu folded into one control, where the header has no room for both. */
function MoreMenu({ seesAgents }: { seesAgents: boolean }) {
  const { role } = useRole();
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger render={<button type="button" />} aria-label="More" className={ICON_LINK}>
        <DotsThreeVertical className="size-5" weight="bold" aria-hidden />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        {seesAgents ? <DropdownMenu.LinkItem render={<Link href="/alerts" />}>Alerts</DropdownMenu.LinkItem> : null}
        <AccountLinks role={role} />
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}

/**
 * The header always renders, whatever is loading: the Stop control, the paper badge, and the way
 * home never wait for workspace data (brief §5, rule 13). Stop never shrinks and never leaves the
 * screen (`e2e/stop-visible.spec.ts`); as the header narrows, lower-priority items give way first:
 * the search becomes an icon (below `lg`), the trail keeps two crumbs and the workspace switcher
 * becomes an icon (below `xl`), alerts and the account menu fold into "More" (below `xl`), the paper
 * badge drops its gloss (below 30rem), and on phones the tab bar and the sidebar sheet carry the rest.
 */
export function AppHeader() {
  const pathname = usePathname();
  const { ws, now } = useRuntime();
  const { role } = useRole();
  const open = ws.approvals.filter((a) => approvalAt(a, now).status === "delivered").length;
  // Deep pages keep their last three crumbs; the page's tabs and the sidebar carry the rest of the way back.
  const crumbs = crumbsFor(pathname, (id) => ws.agents.find((a) => a.agent_id === id)?.label)
    .filter((c) => canOpen(role, c.href))
    .slice(-3);
  const seesAgents = can(role, "agents.view");
  const home = homeFor(role);

  return (
    <header className="sticky top-0 z-30 border-b border-border/70 bg-card">
      <div className="flex h-16 items-center gap-1 px-(--page-x) sm:gap-2">
        <Sidebar.Trigger className="lg:hidden" />
        <Link href={home.href} className="shrink-0 px-1 text-foreground lg:hidden" aria-label={`Owlhead, ${home.label}`}>
          <Wordmark />
        </Link>
        <div className="hidden lg:block">
          <WorkspaceSwitcher />
        </div>
        <div className="hidden min-w-0 flex-1 md:flex">
          <Breadcrumbs size="sm" className={`mr-0 min-w-0 [&_a]:min-w-0 [&_a]:shrink-[4] [&_a>span]:truncate ${crumbs.length > 2 ? TWO_CRUMBS : ""}`}>
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
        <div className="ml-auto flex shrink-0 items-center gap-0.5 sm:gap-1.5">
          <CommandMenu />
          <EnvironmentBadge environment={ws.environment} />
          {seesAgents ? (
            <>
              <Link href="/approvals" aria-label="Approvals" className={`${ICON_LINK} max-sm:hidden`}>
                <Tray className="size-5" aria-hidden />
                <ApprovalsCount n={open} className="absolute top-0.5 right-0" />
              </Link>
              <Link href="/alerts" aria-label="Alerts" className={`${ICON_LINK} max-xl:hidden`}>
                <Bell className="size-5" aria-hidden />
              </Link>
            </>
          ) : null}
          <div className="max-xl:hidden">
            <UserMenu />
          </div>
          <div className="max-sm:hidden xl:hidden">
            <MoreMenu seesAgents={seesAgents} />
          </div>
          <StopControl className="ml-1 sm:ml-2" />
        </div>
      </div>
    </header>
  );
}
