"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { Buildings, CaretRight, GearSix, House, MagnifyingGlass, type Icon as PhosphorIcon, Scroll, X } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { homeFor } from "@/lib/access";
import { useRuntime } from "@/lib/mock-runtime";
import { type Role, can, useRole } from "@/lib/roles";
import { GROUP_LABEL, SCREENS, SECTION_INDEX, type Screen, type ScreenGroup } from "@/lib/screens";
import { FixtureTag } from "@/components/domain/placeholders";
import { WorkspaceSwitcher } from "./app-header";
import { OPEN_COMMAND_EVENT } from "./command-menu";
import { isCurrent } from "./dock";
import { SCREEN_ICON } from "./screen-icons";
import { healthItems } from "./status-strip";
import { ThemeMenu } from "./theme-menu";

/** The phone's own tabs for a role that sees agents; the rest of the app is under More. */
export const PHONE_TAB_LINKS = ["home", "approvals", "agents"] as const;

export interface PhoneTab {
  key: string;
  href: string;
  label: string;
  icon: PhosphorIcon;
}

const TAB_LABEL: Record<(typeof PHONE_TAB_LINKS)[number], string> = { home: "Home", approvals: "Approvals", agents: "Agents" };

/** A role that cannot see agents gets its own home as the one tab beside More. */
export function phoneTabs(role: Role): PhoneTab[] {
  if (can(role, "agents.view")) {
    return PHONE_TAB_LINKS.map((key) => {
      const screen = SCREENS.find((s) => s.key === key)!;
      return { key, href: screen.href, label: TAB_LABEL[key], icon: SCREEN_ICON[key] ?? House };
    });
  }
  const home = homeFor(role);
  const index = home.href === SECTION_INDEX.audit.href ? SECTION_INDEX.audit : SECTION_INDEX.workspace;
  return [{ key: "home", href: index.href, label: index.label, icon: index === SECTION_INDEX.audit ? Scroll : GearSix }];
}

interface MoreLink {
  href: string;
  label: string;
  icon: PhosphorIcon;
}

export interface MoreGroup {
  label: string | null;
  links: MoreLink[];
}

/** Under More the workspace screens read as Settings, the word a phone owner looks for. */
const MORE_LABEL: Record<ScreenGroup, string | null> = { ...GROUP_LABEL, workspace: "Settings" };

const INDEX_LINK: Partial<Record<ScreenGroup, MoreLink>> = {
  audit: { href: SECTION_INDEX.audit.href, label: "Audit overview", icon: Scroll },
  workspace: { href: SECTION_INDEX.workspace.href, label: "All settings", icon: GearSix },
};

function linkOf(s: Screen): MoreLink {
  return { href: s.href, label: s.label, icon: SCREEN_ICON[s.key] ?? House };
}

/**
 * Every screen a role may open that is not on its tabs, in the order of `SCREENS`, under the group
 * labels the dock uses; Audit and Settings open with their overview.
 */
export function moreGroups(role: Role): MoreGroup[] {
  const onTabs = new Set(phoneTabs(role).map((t) => t.href));
  const groups: MoreGroup[] = [];
  const byGroup = new Map<ScreenGroup, MoreGroup>();
  for (const s of SCREENS) {
    if (onTabs.has(s.href) || !can(role, s.needs)) continue;
    let group = byGroup.get(s.group);
    if (!group) {
      const index = INDEX_LINK[s.group];
      group = { label: MORE_LABEL[s.group], links: index && !onTabs.has(index.href) ? [index] : [] };
      byGroup.set(s.group, group);
      groups.push(group);
    }
    group.links.push(linkOf(s));
  }
  return groups;
}

const ROW =
  "press group -mx-2 flex min-h-11 items-center gap-3 rounded-xl px-2 py-2.5 text-left outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset aria-[current=page]:font-semibold";

function Row({ link, pathname, onGo }: { link: MoreLink; pathname: string; onGo: () => void }) {
  const current = isCurrent(pathname, link.href);
  const Icon = link.icon;
  return (
    <li className="border-b border-border/70 last:border-b-0">
      <Link href={link.href} onClick={onGo} aria-current={current ? "page" : undefined} className={ROW}>
        <Icon className="size-5 shrink-0 text-muted-foreground" weight={current ? "fill" : "regular"} aria-hidden />
        <span className="min-w-0 flex-1">{link.label}</span>
        <CaretRight className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      </Link>
    </li>
  );
}

/**
 * More, the phone's fourth tab: a bottom sheet with the account and workspace, Search, and every
 * screen that is not a tab, then the theme and the feeds the status strip used to list. Flat like
 * every sheet (the glass is the frame's alone), hairline rows, 44px targets, clear of the home
 * indicator. Choosing a screen closes it on the way. It is not modal and it and its backdrop stop
 * at the header, so Stop stays in view, in the accessibility tree and one press away while it is open.
 */
export function MoreSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const pathname = usePathname();
  const { role } = useRole();
  const { ws, now } = useRuntime();
  const groups = moreGroups(role);
  const close = () => onOpenChange(false);
  const search = () => {
    close();
    window.dispatchEvent(new Event(OPEN_COMMAND_EVENT));
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange} modal={false}>
      <Dialog.Portal>
        <Dialog.Backdrop data-slot="sheet-backdrop" className="fixed inset-x-0 top-[calc(4rem+1px)] bottom-0 z-50 bg-ink/40" />
        <Dialog.Popup
          data-slot="more-sheet"
          className="fixed inset-x-0 bottom-0 z-50 mx-auto flex max-h-[min(85dvh,calc(100dvh-4rem-1px))] w-full max-w-lg flex-col overflow-y-auto overscroll-contain rounded-t-3xl border-t border-border bg-card pb-[calc(env(safe-area-inset-bottom)+1rem)] text-foreground shadow-2xl outline-none sm:border-x"
        >
          <div className="flex items-center justify-between gap-3 px-5 pt-3 pb-1">
            <Dialog.Title className="text-h2">More</Dialog.Title>
            <Dialog.Close
              aria-label="Close"
              className="-mr-2 grid size-11 place-items-center rounded-full text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
            >
              <X className="size-5" aria-hidden />
            </Dialog.Close>
          </div>
          <Dialog.Description className="sr-only">Your account and workspace, search, and every other screen.</Dialog.Description>

          <div className="grid gap-6 px-5 pt-2">
            <section aria-label="Account and workspace" data-slot="more-account" className="grid gap-1 border-b border-border/70 pb-3">
              <div className="flex min-h-11 min-w-0 items-center gap-3">
                <Buildings className="size-5 shrink-0 text-muted-foreground" aria-hidden />
                <span className="grid min-w-0">
                  <span className="field-label text-muted-foreground">Account</span>
                  <span className="truncate text-sm font-medium">{ws.connection.broker}</span>
                </span>
              </div>
              <WorkspaceSwitcher inSheet className="-mx-2.5 h-11 w-fit max-w-full" />
            </section>

            <button type="button" onClick={search} data-slot="more-search" className={cn(ROW, "mx-0 -mt-3 w-full")}>
              <MagnifyingGlass className="size-5 shrink-0 text-muted-foreground" aria-hidden />
              <span className="min-w-0 flex-1">Search</span>
              <span className="text-caption text-muted-foreground">Agents and screens</span>
            </button>

            {groups.map((g, i) => (
              <section key={g.label ?? `group-${i}`} aria-label={g.label ?? "Screens"} className="grid gap-1">
                {g.label ? <h3 className="field-label text-muted-foreground">{g.label}</h3> : null}
                <ul className="grid">
                  {g.links.map((l) => (
                    <Row key={l.href} link={l} pathname={pathname} onGo={close} />
                  ))}
                </ul>
              </section>
            ))}

            <div className="flex min-h-11 items-center justify-between gap-3 border-t border-border/70 pt-3">
              <span className="text-sm font-medium">Theme</span>
              <ThemeMenu className="press inline-flex size-11 items-center justify-center rounded-full text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring" />
            </div>

            <section aria-label="Feeds" data-slot="more-feeds" className="grid gap-2 border-t border-border/70 pt-3">
              <h3 className="field-label text-muted-foreground">Feeds</h3>
              <ul className="grid gap-1 text-caption text-muted-foreground tabular">
                {healthItems(ws, now).map((item) => (
                  <li key={item.key} data-state={item.state}>
                    {item.text}
                  </li>
                ))}
              </ul>
              <FixtureTag className="w-fit" />
            </section>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
