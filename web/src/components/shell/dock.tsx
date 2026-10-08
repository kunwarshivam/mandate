"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { DropdownMenu } from "@cloudflare/kumo/components/dropdown";
import { BuildingCommunity } from "pixelarticons/react/BuildingCommunity.js";
import { Gear } from "pixelarticons/react/Gear.js";
import { Home } from "pixelarticons/react/Home.js";
import { MoreHorizontal } from "pixelarticons/react/MoreHorizontal.js";
import { Script } from "pixelarticons/react/Script.js";
import { type Icon, menuIcon } from "@/components/icon";
import { useRuntime } from "@/lib/mock-runtime";
import { type Role, can, useRole } from "@/lib/roles";
import { GROUP_LABEL, GROUP_NEEDS, SCREENS, SECTION_INDEX, type Screen, type ScreenGroup, listedScreens } from "@/lib/screens";
import { SCREEN_ICON } from "./screen-icons";
import { StopButton } from "./stop-control";

/** The screens that sit on the dock itself, in order; every other screen is one menu away. */
export const DOCK_LINKS = ["home", "messages", "approvals", "alerts", "agents", "positions"] as const;

/** One menu, More, holds every listed screen that is not on the dock (DEC-513 folded Audit into it). */
export type DockMenu = "more";

export function menuFor(group: ScreenGroup): DockMenu {
  switch (group) {
    case "main":
    case "agents":
    case "accounts":
    case "audit":
    case "workspace":
      return "more";
    default: {
      const unhandled: never = group;
      throw new Error(`unhandled group ${String(unhandled)}`);
    }
  }
}

interface MenuLink {
  href: string;
  label: string;
  icon: Icon;
}

interface MenuGroup {
  label: string | null;
  links: MenuLink[];
}

const INDEX_ICON: Record<"audit" | "workspace", Icon> = { audit: Script, workspace: Gear };

function linkOf(s: Screen): MenuLink {
  return { href: s.href, label: s.label, icon: SCREEN_ICON[s.key] ?? Home };
}

/**
 * A menu's groups for a role: each listed screen group in the order of `SCREENS`, the audit and
 * workspace groups opening with their overview. A section whose screens are all still to come
 * keeps its overview alone, which names what is coming.
 */
export function menuGroups(menu: DockMenu, role: Role): MenuGroup[] {
  const docked = new Set<string>(DOCK_LINKS);
  const screens = listedScreens().filter((s) => !docked.has(s.key) && menuFor(s.group) === menu && can(role, s.needs));
  const groups: MenuGroup[] = [];
  for (const s of screens) {
    const label = GROUP_LABEL[s.group];
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.links.push(linkOf(s));
    else groups.push({ label, links: [linkOf(s)] });
  }
  for (const index of ["audit", "workspace"] as const) {
    if (!can(role, GROUP_NEEDS[index])) continue;
    const overview = { href: SECTION_INDEX[index].href, label: `${SECTION_INDEX[index].label} overview`, icon: INDEX_ICON[index] };
    const home = groups.find((g) => g.label === GROUP_LABEL[index]);
    if (home) home.links.unshift(overview);
    else groups.push({ label: GROUP_LABEL[index], links: [overview] });
  }
  return groups;
}

export function isCurrent(pathname: string, href: string): boolean {
  switch (href) {
    case "/":
    case SECTION_INDEX.audit.href:
    case SECTION_INDEX.workspace.href:
      return pathname === href;
    case "/agents":
      return pathname === "/agents" || (pathname.startsWith("/agents/") && !pathname.startsWith("/agents/new"));
    default:
      return pathname === href || pathname.startsWith(`${href}/`);
  }
}

/** The dock's own names, where a screen's full name is longer than a dock label needs to be. */
const DOCK_LABEL: Partial<Record<string, string>> = { agents: "Agents" };

const ITEM =
  "group press relative flex h-12.5 min-w-16 shrink-0 flex-col items-center justify-center gap-0.5 rounded-2xl px-2 text-muted-foreground outline-none hover:bg-(--dock-hover) hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring data-current:bg-(--dock-current) data-current:text-foreground forced-colors:data-current:outline-2 forced-colors:data-current:outline-solid";

/** The label, with room kept for its semibold weight so the dock does not shift when the current section changes. */
function Label({ children }: { children: string }) {
  return (
    <span className="grid text-xs whitespace-nowrap">
      <span data-slot="dock-label" className="col-start-1 row-start-1 font-medium group-data-current:font-semibold">
        {children}
      </span>
      <span aria-hidden className="invisible col-start-1 row-start-1 font-semibold">
        {children}
      </span>
    </span>
  );
}

function Count({ n }: { n: number }) {
  if (n === 0) return null;
  return (
    <span
      data-slot="approvals-count"
      className="absolute top-0.5 left-[calc(50%+0.125rem)] inline-flex h-4.5 min-w-4.5 items-center justify-center rounded-sm bg-lapis px-1 font-mono text-[0.6875rem] font-semibold text-lapis-foreground tabular ring-2 ring-card"
    >
      {n}
      <span className="sr-only"> open</span>
    </span>
  );
}

function DockLink({ screen, current, count }: { screen: Screen; current: boolean; count: number }) {
  const Glyph = SCREEN_ICON[screen.key] ?? Home;
  const label = DOCK_LABEL[screen.key] ?? screen.label;
  return (
    <Link href={screen.href} aria-current={current ? "page" : undefined} data-current={current ? "" : undefined} className={ITEM}>
      <Glyph className="size-6" aria-hidden />
      <Label>{label}</Label>
      {screen.key === "approvals" ? <Count n={count} /> : null}
    </Link>
  );
}

function DockMenuButton({ label, icon: Glyph, current, children }: { label: string; icon: Icon; current: boolean; children: ReactNode }) {
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger
        render={<button type="button" />}
        aria-current={current ? "true" : undefined}
        data-current={current ? "" : undefined}
        className={ITEM}
      >
        <Glyph className="size-6" aria-hidden />
        <Label>{label}</Label>
      </DropdownMenu.Trigger>
      <DropdownMenu.Content side="top" align="center" sideOffset={12} className="min-w-56">
        {children}
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}

function MenuLinks({ groups, pathname }: { groups: MenuGroup[]; pathname: string }) {
  return groups.map((g, i) => (
    <DropdownMenu.Group key={g.label ?? `group-${i}`}>
      {i > 0 ? <DropdownMenu.Separator /> : null}
      {g.label ? <DropdownMenu.Label>{g.label}</DropdownMenu.Label> : null}
      {g.links.map((l) => {
        const current = isCurrent(pathname, l.href);
        return (
          <DropdownMenu.LinkItem key={l.href} render={<Link href={l.href} />} icon={menuIcon(l.icon)} aria-current={current ? "page" : undefined}>
            {l.label}
          </DropdownMenu.LinkItem>
        );
      })}
    </DropdownMenu.Group>
  ));
}

/**
 * The desktop navigation (from 64rem): a floating glass dock centred at the bottom of the viewport,
 * in place of the sidebar. Every item is an icon over its name, and the current section sits on a
 * pill. The owner's everyday screens sit on it; Audit and More open menus with every other screen,
 * so nothing the sidebar reached is lost, and the account the sidebar's header named heads More.
 * Stop ends it, past a divider, so the one control that acts on the account sits apart from the
 * places to go (DEC-452). The dock sits in the page's bottom padding and scroll padding, so it never
 * covers content, a focused control or an approval's pinned choices, and it never reaches the header.
 */
export function Dock({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  const { role } = useRole();
  const { ws } = useRuntime();
  const links = DOCK_LINKS.map((key) => SCREENS.find((s) => s.key === key)!).filter((s) => can(role, s.needs));
  const more = menuGroups("more", role);
  const within = (groups: MenuGroup[]) => groups.some((g) => g.links.some((l) => isCurrent(pathname, l.href)));

  return (
    <nav
      aria-label="Primary"
      data-slot="dock"
      className="glass flex items-center gap-1 rounded-3xl border p-1.5 shadow-md [--glass-edge:var(--dock-edge)] [--glass:var(--dock-glass)]"
    >
      {links.map((s) => (
        <DockLink key={s.key} screen={s} current={isCurrent(pathname, s.href)} count={approvals} />
      ))}
      {links.length > 0 ? <span aria-hidden data-slot="dock-divider" className="mx-1 h-8 w-px bg-(--dock-edge)" /> : null}
      <DockMenuButton label="More" icon={MoreHorizontal} current={within(more) || pathname.startsWith("/audit/") || pathname.startsWith("/settings/")}>
        <DropdownMenu.Group>
          <DropdownMenu.Label>Account</DropdownMenu.Label>
          <DropdownMenu.Item icon={menuIcon(BuildingCommunity)} selected>
            {ws.connection.broker}
          </DropdownMenu.Item>
        </DropdownMenu.Group>
        {more.length > 0 ? <DropdownMenu.Separator /> : null}
        <MenuLinks groups={more} pathname={pathname} />
      </DockMenuButton>
      {can(role, "stop.open") ? (
        <>
          <span aria-hidden data-slot="dock-stop-divider" className="mx-1 h-8 w-px bg-(--dock-edge)" />
          <StopButton place="dock" />
        </>
      ) : null}
    </nav>
  );
}
