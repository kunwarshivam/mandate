"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { DropdownMenu } from "@cloudflare/kumo/components/dropdown";
import { Buildings, DotsThree, GearSix, House, type Icon as PhosphorIcon, Scroll } from "@phosphor-icons/react";
import { useRuntime } from "@/lib/mock-runtime";
import { type Role, can, useRole } from "@/lib/roles";
import { GROUP_LABEL, SCREENS, SECTION_INDEX, type Screen, type ScreenGroup } from "@/lib/screens";
import { SCREEN_ICON } from "./screen-icons";

/** The screens that sit on the dock itself, in order; every other screen is one menu away. */
export const DOCK_LINKS = ["home", "approvals", "alerts", "agents", "positions", "connections"] as const;

export type DockMenu = "audit" | "more";

export function menuFor(group: ScreenGroup): DockMenu {
  switch (group) {
    case "audit":
      return "audit";
    case "main":
    case "agents":
    case "accounts":
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
  icon: PhosphorIcon;
}

interface MenuGroup {
  label: string | null;
  links: MenuLink[];
}

const INDEX_ICON: Record<"audit" | "workspace", PhosphorIcon> = { audit: Scroll, workspace: GearSix };

function linkOf(s: Screen): MenuLink {
  return { href: s.href, label: s.label, icon: SCREEN_ICON[s.key] ?? House };
}

/** A menu's groups for a role: its section index first, then each screen group in the order of `SCREENS`. */
export function menuGroups(menu: DockMenu, role: Role): MenuGroup[] {
  const docked = new Set<string>(DOCK_LINKS);
  const screens = SCREENS.filter((s) => !docked.has(s.key) && menuFor(s.group) === menu && can(role, s.needs));
  const groups: MenuGroup[] = [];
  for (const s of screens) {
    const label = GROUP_LABEL[s.group];
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.links.push(linkOf(s));
    else groups.push({ label, links: [linkOf(s)] });
  }
  const index = menu === "audit" ? "audit" : "workspace";
  const home = groups.find((g) => g.label === GROUP_LABEL[index]);
  if (home) home.links.unshift({ href: SECTION_INDEX[index].href, label: `${SECTION_INDEX[index].label} overview`, icon: INDEX_ICON[index] });
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
  const Icon = SCREEN_ICON[screen.key] ?? House;
  const label = DOCK_LABEL[screen.key] ?? screen.label;
  return (
    <Link href={screen.href} aria-current={current ? "page" : undefined} data-current={current ? "" : undefined} className={ITEM}>
      <Icon className="size-5" weight={current ? "fill" : "regular"} aria-hidden />
      <Label>{label}</Label>
      {screen.key === "approvals" ? <Count n={count} /> : null}
    </Link>
  );
}

function DockMenuButton({ label, icon: Icon, current, children }: { label: string; icon: PhosphorIcon; current: boolean; children: ReactNode }) {
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger
        render={<button type="button" />}
        aria-current={current ? "true" : undefined}
        data-current={current ? "" : undefined}
        className={ITEM}
      >
        <Icon className="size-5" weight={current ? "fill" : "regular"} aria-hidden />
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
          <DropdownMenu.LinkItem key={l.href} render={<Link href={l.href} />} icon={l.icon} aria-current={current ? "page" : undefined}>
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
 * The dock sits in the page's bottom padding and scroll padding, so it never covers content, a
 * focused control or an approval's pinned choices, and it never reaches the header.
 */
export function Dock({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  const { role } = useRole();
  const { ws } = useRuntime();
  const links = DOCK_LINKS.map((key) => SCREENS.find((s) => s.key === key)!).filter((s) => can(role, s.needs));
  const audit = menuGroups("audit", role);
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
      {audit.length > 0 ? (
        <DockMenuButton label="Audit" icon={Scroll} current={pathname === "/audit" || pathname.startsWith("/audit/")}>
          <MenuLinks groups={audit} pathname={pathname} />
        </DockMenuButton>
      ) : null}
      <DockMenuButton label="More" icon={DotsThree} current={within(more)}>
        <DropdownMenu.Group>
          <DropdownMenu.Label>Account</DropdownMenu.Label>
          <DropdownMenu.Item icon={Buildings} selected>
            {ws.connection.broker}
          </DropdownMenu.Item>
        </DropdownMenu.Group>
        {more.length > 0 ? <DropdownMenu.Separator /> : null}
        <MenuLinks groups={more} pathname={pathname} />
      </DockMenuButton>
    </nav>
  );
}
