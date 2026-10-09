"use client";

import type { RefObject } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Breadcrumbs } from "@cloudflare/kumo/components/breadcrumbs";
import { DropdownMenu } from "@cloudflare/kumo/components/dropdown";
import { AvatarCircle } from "pixelarticons/react/AvatarCircle.js";
import { Bell } from "pixelarticons/react/Bell.js";
import { ChevronsVertical } from "pixelarticons/react/ChevronsVertical.js";
import { Logout } from "pixelarticons/react/Logout.js";
import { MoreVertical } from "pixelarticons/react/MoreVertical.js";
import { Suitcase } from "pixelarticons/react/Suitcase.js";
import { CopilotButton } from "@/components/copilot/copilot";
import { menuIcon } from "@/components/icon";
import { canOpen, homeFor } from "@/lib/access";
import { useRuntime } from "@/lib/mock-runtime";
import { type Role, can, useRole } from "@/lib/roles";
import { type Crumb, crumbsFor } from "@/lib/screens";
import { signOut, useSession } from "@/lib/session";
import { cn } from "@/lib/utils";
import { Wordmark } from "./brand";
import { CommandMenu } from "./command-menu";
import { EnvironmentBadge } from "./environment-badge";
import { ThemeMenu } from "./theme-menu";

/** Fixture workspaces: the switcher shows the shape of the control, and only one is connected. */
const WORKSPACES = [
  { id: "ws_paper", label: "Paper workspace", current: true },
  { id: "ws_second", label: "Second workspace", current: false },
] as const;

const ICON_LINK =
  "press relative inline-flex size-11 shrink-0 items-center justify-center rounded-lg text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring";

/**
 * Where the trail is narrower than 20rem it keeps only the current page, which never truncates, and
 * the earlier crumbs fold into a menu. A top-level screen has only Home before it, which the brand
 * already links to, so it folds nothing and shows no "…". Kumo renders the full trail as the nav's
 * last child.
 */
const CURRENT_ONLY = "@max-[20rem]:[&>div:last-child>*:not(:last-child)]:hidden";

function EarlierCrumbs({ crumbs }: { crumbs: Crumb[] }) {
  return (
    <div className="flex shrink-0 items-center @min-[20rem]:hidden">
      <DropdownMenu>
        <DropdownMenu.Trigger
          render={<button type="button" />}
          aria-label="Earlier pages"
          className="press inline-flex h-10 min-w-8 items-center justify-center rounded-md px-1.5 text-sm text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
        >
          …
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="start">
          {crumbs.map((c) => (
            <DropdownMenu.LinkItem key={c.href} render={<Link href={c.href} />}>
              {c.label}
            </DropdownMenu.LinkItem>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu>
      <Breadcrumbs.Separator />
    </div>
  );
}

/**
 * The header's switcher from `lg`, an icon below 100rem (the band where the paper badge also runs
 * compact, so the trail keeps its room), and at the head of the phone's More sheet, where it
 * always reads in full.
 */
export function WorkspaceSwitcher({ inSheet = false, className }: { inSheet?: boolean; className?: string }) {
  const current = WORKSPACES.find((w) => w.current) ?? WORKSPACES[0];
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger
        render={<button type="button" />}
        aria-label={`Workspace: ${current.label}`}
        className={cn(
          "press inline-flex h-10 max-w-48 shrink-0 items-center gap-1.5 rounded-md px-2.5 text-sm font-medium outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring",
          className,
        )}
      >
        <Suitcase className={cn("size-6 shrink-0", inSheet ? "text-muted-foreground" : "min-[100rem]:hidden")} aria-hidden />
        <span className={cn("truncate", !inSheet && "max-[100rem]:sr-only")}>{current.label}</span>
        <ChevronsVertical className="size-6 shrink-0 text-muted-foreground" aria-hidden />
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

/** Base UI's menu labels must sit inside a group. Sign out shows only while someone is signed in (DEC-211). */
function AccountLinks({ role }: { role: Role }) {
  const session = useSession();
  return (
    <>
      <DropdownMenu.Group>
        <DropdownMenu.Label data-slot="account-identity" className="max-w-72">
          You, {role}
          {session?.email ? <span className="block truncate font-normal text-muted-foreground">{session.email}</span> : null}
        </DropdownMenu.Label>
        {can(role, "workspace.view") ? (
          <>
            <DropdownMenu.LinkItem render={<Link href="/settings/profile" />}>Profile</DropdownMenu.LinkItem>
            <DropdownMenu.LinkItem render={<Link href="/settings/notifications" />}>Notifications</DropdownMenu.LinkItem>
          </>
        ) : (
          <DropdownMenu.LinkItem render={<Link href="/audit" />}>Audit</DropdownMenu.LinkItem>
        )}
      </DropdownMenu.Group>
      {session ? (
        <>
          <DropdownMenu.Separator />
          <DropdownMenu.Group>
            <DropdownMenu.Item icon={menuIcon(Logout)} onClick={() => void signOut()}>
              Sign out
            </DropdownMenu.Item>
          </DropdownMenu.Group>
        </>
      ) : null}
    </>
  );
}

function UserMenu() {
  const { role } = useRole();
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger render={<button type="button" />} aria-label="Your account" className={ICON_LINK}>
        <AvatarCircle className="size-6" aria-hidden />
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
      <DropdownMenu.Trigger render={<button type="button" />} aria-label="Alerts and account" className={ICON_LINK}>
        <MoreVertical className="size-6" aria-hidden />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        {seesAgents ? <DropdownMenu.LinkItem render={<Link href="/alerts" />}>Alerts</DropdownMenu.LinkItem> : null}
        <AccountLinks role={role} />
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}

/**
 * The header always renders, whatever is loading: the paper badge and the way home never wait for
 * workspace data (brief §5). Stop is not here: it ends the dock and the phone's tab bar (DEC-452).
 * Nor is Approvals: the dock and the tab bar carry it and its one count (DEC-467).
 * As the header narrows, lower-priority items give way first:
 * the trail folds its earlier crumbs into a menu and then hides, never truncating the current page,
 * the command bar narrows (it is centred where both sides fit, `e2e/command-bar.spec.ts`), the
 * workspace switcher becomes an icon (below 100rem), alerts and the account menu fold into "Alerts
 * and account" (below `xl`), and the paper badge keeps its gloss for screen readers only (at `lg`
 * below 100rem, and below 30rem). Below `lg` the header holds two things, the brand owl and the
 * paper badge (DEC-207, DEC-452); the tab bar and its More sheet carry the rest. There, an open
 * thread hides it through `className` and carries the paper badge in its own bar (DEC-482).
 */
export function AppHeader({
  className,
  sheetLayer,
}: {
  className?: string;
  /** Where the command palette mounts: the frame's sheet layer, under the dock and the tab bar (C-21). */
  sheetLayer?: RefObject<HTMLElement | null>;
}) {
  const pathname = usePathname();
  const { ws } = useRuntime();
  const { role } = useRole();
  // Deep pages keep their last three crumbs; the page's tabs and the dock carry the rest of the way back.
  const crumbs = crumbsFor(pathname, (id) => ws.agents.find((a) => a.agent_id === id)?.label)
    .filter((c) => canOpen(role, c.href))
    .slice(-3);
  const seesAgents = can(role, "agents.view");
  const home = homeFor(role);

  return (
    <header data-slot="app-header" className={cn("glass sticky top-0 z-30 shrink-0 border-b", className)}>
      <div className="flex h-16 items-center gap-1 overflow-hidden px-(--page-x) whitespace-nowrap sm:gap-2 lg:gap-4">
        <div className="flex min-w-0 flex-1 items-center gap-1 sm:gap-2 lg:min-w-auto lg:basis-0">
          <Link
            href={home.href}
            className="inline-flex min-h-11 min-w-11 shrink-0 items-center justify-center px-1 text-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring lg:mr-2"
            aria-label={`Owlhead, ${home.label}`}
          >
            <Wordmark />
          </Link>
          <div className="hidden lg:block">
            <WorkspaceSwitcher />
          </div>
          <div className="@container hidden min-w-0 flex-1 items-center lg:flex [&>*]:@max-[10rem]:hidden">
            {crumbs.length > 2 ? <EarlierCrumbs crumbs={crumbs.slice(0, -1)} /> : null}
            <Breadcrumbs size="sm" className={`mr-0 min-w-0 [&_[aria-current=page]]:shrink-0 [&_a]:min-w-0 [&_a]:shrink-[4] [&_a>span]:truncate ${CURRENT_ONLY}`}>
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
        </div>
        <CommandMenu container={sheetLayer} />
        <div className="flex shrink-0 items-center gap-0.5 sm:gap-1.5 lg:flex-1 lg:basis-0 lg:justify-end">
          {seesAgents ? <CopilotButton className="max-lg:hidden" /> : null}
          <ThemeMenu className={`${ICON_LINK} max-lg:hidden`} />
          <EnvironmentBadge environment={ws.environment} className="lg:max-[100rem]:[&>span+span]:sr-only" />
          {seesAgents ? (
            <Link href="/alerts" aria-label="Alerts" className={`${ICON_LINK} max-xl:hidden`}>
              <Bell className="size-6" aria-hidden />
            </Link>
          ) : null}
          <div className="max-xl:hidden">
            <UserMenu />
          </div>
          <div className="max-lg:hidden xl:hidden">
            <MoreMenu seesAgents={seesAgents} />
          </div>
        </div>
      </div>
    </header>
  );
}
