"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Home } from "pixelarticons/react/Home.js";
import { can, useRole } from "@/lib/roles";
import { SCREENS, type Screen } from "@/lib/screens";
import { SCREEN_ICON } from "./screen-icons";
import { StopButton } from "./stop-control";

/** The screens that sit on the dock itself, in order: every listed screen (DEC-2293, DEC-2413). */
export const DOCK_LINKS = ["home", "portfolio", "new", "settings"] as const;

export function isCurrent(pathname: string, href: string): boolean {
  switch (href) {
    case "/":
      return pathname === "/" || (pathname.startsWith("/agents/") && pathname !== "/agents/new");
    case "/settings":
      return pathname === "/settings" || pathname.startsWith("/connections/");
    default:
      return pathname === href || pathname.startsWith(`${href}/`);
  }
}

/** The dock's own names, where a screen's full name is longer than a dock label needs to be. */
const DOCK_LABEL: Partial<Record<string, string>> = { new: "New" };

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
      {screen.key === "home" ? <Count n={count} /> : null}
    </Link>
  );
}

/**
 * The desktop navigation (from 64rem): a floating glass dock centred at the bottom of the viewport,
 * in place of the sidebar. Every item is an icon over its name, and the current section sits on a
 * pill: Home, New agent and Settings, the whole app (DEC-2293). Home carries the count of requests
 * waiting for you. Stop ends it, past a divider, so the one control that acts on the account sits apart from the
 * places to go (DEC-452). The dock sits in the page's bottom padding and scroll padding, so it never
 * covers content, a focused control or an approval's pinned choices, and it never reaches the header.
 */
export function Dock({ approvals }: { approvals: number }) {
  const pathname = usePathname();
  const { role } = useRole();
  const links = DOCK_LINKS.map((key) => SCREENS.find((s) => s.key === key)!).filter((s) => can(role, s.needs));

  return (
    <nav
      aria-label="Primary"
      data-slot="dock"
      className="glass flex items-center gap-1 rounded-3xl border p-1.5 shadow-md [--glass-edge:var(--dock-edge)] [--glass:var(--dock-glass)]"
    >
      {links.map((s) => (
        <DockLink key={s.key} screen={s} current={isCurrent(pathname, s.href)} count={approvals} />
      ))}
      {can(role, "stop.open") ? (
        <>
          <span aria-hidden data-slot="dock-stop-divider" className="mx-1 h-8 w-px bg-(--dock-edge)" />
          <StopButton place="dock" />
        </>
      ) : null}
    </nav>
  );
}
