"use client";

import { type FormEvent, type KeyboardEvent, type MouseEvent, type ReactNode, type RefObject, createContext, useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowLeft } from "pixelarticons/react/ArrowLeft.js";
import { ArrowRight } from "pixelarticons/react/ArrowRight.js";
import { Folder } from "pixelarticons/react/Folder.js";
import { Hand } from "pixelarticons/react/Hand.js";
import { Home } from "pixelarticons/react/Home.js";
import { Image as ImageIcon } from "pixelarticons/react/Image.js";
import { Printer } from "pixelarticons/react/Printer.js";
import { Reload } from "pixelarticons/react/Reload.js";
import { Search } from "pixelarticons/react/Search.js";
import { cn } from "@/lib/utils";
import { ETCHED, MENU_ITEM, MENU_PANEL, MONO, PIXEL, RAISED, SUNKEN } from "./letter";
import { OpenAppContext } from "./open-app";
import { type AppId, TASK } from "./windows";

/** What the browser's own controls can do to the page in front: go back or forward, go home, or load it again. */
type Act = "back" | "forward" | "home" | "reload";

/**
 * The browser's controls for the tab in front, when it brings its own (DEC-906): the app's tab goes
 * back and forward through its own screens. `null` greys a control out.
 */
export type Nav = { back: (() => void) | null; forward: (() => void) | null; home: () => void; reload: () => void };

export const BrowserNav = createContext<Nav | null>(null);

/**
 * One line of a menu: a part of the page, a window on the desktop, one of the browser's own acts, or
 * a line the page has no use for, greyed out as a browser greyed what it could not do just then.
 */
type Item = { label: string; href: string } | { label: string; app: AppId } | { label: string; act: Act } | { label: string; off: true } | "rule";

/** A menu with no lines is greyed out in the bar. */
type Menu = { label: string; items: Item[] };

/** A guide in the row under the address: a part of the page, or a window on the desktop. */
export type Guide = { label: string; href: string } | { label: string; app: AppId };

const WINDOW_MENU: AppId[] = ["record", "questions", "guestbook", "readme", "owl", "tour", "bin"];

function menus(bookmarks: { id: string; title: string }[], directory: Guide[]): Menu[] {
  return [
    { label: "File", items: [] },
    { label: "Edit", items: [] },
    { label: "View", items: [{ label: "Reload", act: "reload" }, { label: "Change wallpaper…", app: "display" }] },
    { label: "Go", items: [{ label: "Back", act: "back" }, { label: "Forward", act: "forward" }, { label: "Home", act: "home" }] },
    { label: "Bookmarks", items: bookmarks.map((b) => ({ label: b.title, href: `#${b.id}` })) },
    { label: "Options", items: [] },
    { label: "Directory", items: directory },
    { label: "Window", items: WINDOW_MENU.map((app) => ({ label: TASK[app], app })) },
    { label: "Help", items: [{ label: "Handbook", href: "#how" }, { label: "Questions", app: "questions" }, "rule", { label: "About Owlhead…", app: "readme" }] },
  ];
}

/**
 * The browser's own acts on the tab in front, and whether each can act now. With no tab of its own
 * in front, Home takes the page in the same window to its top and Reload loads it again, and there
 * is nowhere to go back or forward to.
 */
function useActs(from: RefObject<HTMLElement | null>): { run: (act: Act) => void; can: (act: Act) => boolean } {
  const nav = useContext(BrowserNav);
  const can = (act: Act) => (act === "back" ? !!nav?.back : act === "forward" ? !!nav?.forward : true);
  const run = (act: Act) => {
    switch (act) {
      case "back":
        nav?.back?.();
        return;
      case "forward":
        nav?.forward?.();
        return;
      case "home": {
        if (nav) return nav.home();
        const root = from.current?.closest("[data-slot=browser]")?.querySelector<HTMLElement>("[data-scroll-root]");
        const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        root?.scrollTo({ top: 0, behavior: still ? "auto" : "smooth" });
        return;
      }
      case "reload":
        if (nav) return nav.reload();
        window.location.reload();
        return;
      default: {
        const unhandled: never = act;
        throw new Error(`unhandled browser act ${String(unhandled)}`);
      }
    }
  };
  return { run, can };
}

/** Up and Down move through a menu's lines, skipping the greyed ones. */
function step(menu: HTMLElement, by: 1 | -1) {
  const items = [...menu.querySelectorAll<HTMLElement>("[role=menuitem]:not([aria-disabled=true])")];
  const at = items.indexOf(document.activeElement as HTMLElement);
  items[(at + by + items.length) % items.length]?.focus();
}

/**
 * The menu bar, as a browser of 1996 had it. A menu opens on a press and closes on a choice, a press
 * elsewhere or Escape; with one open, pointing at another title opens that one instead, and the
 * arrow keys move through lines and across menus. Like the title bar's buttons, the titles are
 * pointer affordances out of the tab cycle; every line here is also on the page or the desktop.
 */
export function MenuBar({ bookmarks, directory }: { bookmarks: { id: string; title: string }[]; directory: Guide[] }) {
  const all = menus(bookmarks, directory);
  const [open, setOpen] = useState<string | null>(null);
  const [left, setLeft] = useState(0);
  const bar = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const openApp = useContext(OpenAppContext);
  const { run: act, can } = useActs(bar);
  const live = all.filter((m) => m.items.length > 0).map((m) => m.label);

  useEffect(() => {
    if (!open) return;
    const shut = (e: Event) => {
      if (e instanceof globalThis.KeyboardEvent) {
        if (e.key !== "Escape") return;
        bar.current?.querySelector<HTMLElement>(`[data-menu="${open}"]`)?.focus();
        setOpen(null);
      } else if (!(e.target instanceof Node && bar.current?.contains(e.target))) setOpen(null);
    };
    document.addEventListener("keydown", shut);
    document.addEventListener("pointerdown", shut);
    return () => {
      document.removeEventListener("keydown", shut);
      document.removeEventListener("pointerdown", shut);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (!open || !bar.current || !list.current) return;
    const title = bar.current.querySelector<HTMLElement>(`[data-menu="${open}"]`);
    if (!title) return;
    setLeft(Math.max(0, Math.min(title.offsetLeft, bar.current.clientWidth - list.current.offsetWidth)));
    list.current.querySelector<HTMLElement>("[role=menuitem]:not([aria-disabled=true])")?.focus({ preventScroll: true });
  }, [open]);

  const across = (by: 1 | -1) => {
    if (!open) return;
    setOpen(live[(live.indexOf(open) + by + live.length) % live.length]);
  };

  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    const keyed: Record<string, () => void> = {
      ArrowDown: () => step(e.currentTarget, 1),
      ArrowUp: () => step(e.currentTarget, -1),
      ArrowRight: () => across(1),
      ArrowLeft: () => across(-1),
      Tab: () => setOpen(null),
    };
    const run = keyed[e.key];
    if (!run) return;
    if (e.key !== "Tab") e.preventDefault();
    run();
  };

  const choose = (item: Exclude<Item, "rule">, e: MouseEvent<HTMLButtonElement>) => {
    setOpen(null);
    if ("app" in item) openApp(item.app, e);
    if ("act" in item) act(item.act);
  };

  return (
    <div ref={bar} role="menubar" aria-label="Browser menus" data-slot="browser-menus" className={cn("relative flex shrink-0 gap-0.5 overflow-x-clip px-1 py-0.5 text-[0.9375rem] whitespace-nowrap", PIXEL)}>
      {all.map((m) => {
        const mnemonic = (
          <>
            <span className="underline">{m.label[0]}</span>
            {m.label.slice(1)}
          </>
        );
        if (m.items.length === 0)
          return (
            <span key={m.label} role="menuitem" aria-disabled="true" tabIndex={-1} className={cn("px-1.5", ETCHED)}>
              {mnemonic}
            </span>
          );
        const on = open === m.label;
        return (
          <span key={m.label} role="none" className="contents">
            <button
              type="button"
              role="menuitem"
              tabIndex={-1}
              aria-haspopup="menu"
              aria-expanded={on}
              data-menu={m.label}
              onClick={() => setOpen(on ? null : m.label)}
              onPointerEnter={() => open && !on && setOpen(m.label)}
              className={cn("cursor-pointer px-1.5 outline-none", on ? "bg-foreground text-card" : "hover:bg-foreground hover:text-card")}
            >
              {mnemonic}
            </button>
            {on && (
              <div ref={list} role="menu" aria-label={m.label} onKeyDown={keys} className={cn(MENU_PANEL, "absolute top-full z-30 grid min-w-48")} style={{ left }} data-slot="browser-menu">
                {m.items.map((item, i) =>
                  item === "rule" ? (
                    <div key={`rule-${i}`} role="separator" className="mx-1 my-1 border-t border-b border-t-foreground/40 border-b-card" />
                  ) : "off" in item || ("act" in item && !can(item.act)) ? (
                    <span key={item.label} role="menuitem" aria-disabled="true" tabIndex={-1} className={cn(MENU_ITEM, ETCHED, "hover:bg-transparent hover:text-foreground/40")}>
                      {item.label}
                    </span>
                  ) : "href" in item ? (
                    <a key={item.label} role="menuitem" tabIndex={-1} href={item.href} onClick={() => setOpen(null)} className={MENU_ITEM}>
                      {item.label}
                    </a>
                  ) : (
                    <button key={item.label} type="button" role="menuitem" tabIndex={-1} onClick={(e) => choose(item, e)} className={MENU_ITEM}>
                      {item.label}
                    </button>
                  ),
                )}
              </div>
            )}
          </span>
        );
      })}
    </div>
  );
}

const ICON = "size-6";

/**
 * The toolbar. Home takes the page back to its top and Reload loads it again; the rest are greyed,
 * as they were on a page that had just finished loading, with nothing to go back or forward to,
 * nothing still loading to stop, and no dialog behind Open, Print or Find. Stop is named for what
 * it stopped, loading, so it is never read as the product's Stop.
 */
const TOOLS: { label: string; name?: string; icon: ReactNode; act?: Act; wide?: boolean }[] = [
  { label: "Back", icon: <ArrowLeft className={ICON} />, act: "back" },
  { label: "Forward", icon: <ArrowRight className={ICON} />, act: "forward" },
  { label: "Home", icon: <Home className={ICON} />, act: "home" },
  { label: "Reload", icon: <Reload className={ICON} />, act: "reload" },
  { label: "Images", icon: <ImageIcon className={ICON} />, wide: true },
  { label: "Open", icon: <Folder className={ICON} />, wide: true },
  { label: "Print", icon: <Printer className={ICON} />, wide: true },
  { label: "Find", icon: <Search className={ICON} />, wide: true },
  { label: "Stop", name: "Stop loading", icon: <Hand className={ICON} />, wide: true },
];

const TOOL = "grid w-[4.25rem] justify-items-center gap-0.5 bg-muted px-1 py-1 text-[0.8125rem] leading-none";

/** Its buttons are pointer affordances out of the tab cycle, as the title bar's are. */
export function Toolbar() {
  const ref = useRef<HTMLDivElement>(null);
  const { run: act, can } = useActs(ref);
  return (
    <div ref={ref} role="toolbar" aria-label="Browser" className={cn("flex flex-wrap gap-1", PIXEL)} data-slot="browser-tools">
      {TOOLS.map(({ label, name, icon, act: may, wide }) => {
        const does = may && can(may) ? may : undefined;
        return (
        <button
          key={label}
          type="button"
          aria-label={name}
          tabIndex={-1}
          disabled={!does}
          onClick={does ? () => act(does) : undefined}
          data-tool={label}
          className={cn(RAISED, TOOL, does ? "cursor-pointer outline-none active:border-t-foreground/60 active:border-l-foreground/60 active:border-r-card active:border-b-card" : ETCHED, wide && "hidden sm:grid")}
        >
          {icon}
          {label}
        </button>
        );
      })}
    </div>
  );
}

const LOCATION = cn(SUNKEN, "h-8 min-w-0 flex-1 bg-card px-2 text-lg leading-tight text-foreground outline-none selection:bg-foreground selection:text-card", MONO);

/**
 * The address, in a field that selects all of it when pressed, so it is one copy away. Given `onGo`,
 * it takes an address too, typed and sent with Enter, as the app's tab does (DEC-906).
 */
export function LocationField({ address, onGo }: { address: string; onGo?: (typed: string) => void }) {
  const [typed, setTyped] = useState<string | null>(null);
  if (!onGo)
    return <input readOnly tabIndex={-1} aria-label="Address" value={address} onFocus={(e) => e.currentTarget.select()} data-slot="location" className={LOCATION} />;
  const go = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (typed !== null) onGo(typed);
    setTyped(null);
  };
  return (
    <form onSubmit={go} className="flex min-w-0 flex-1" data-slot="location-form">
      <input
        aria-label="Address"
        value={typed ?? address}
        onChange={(e) => setTyped(e.currentTarget.value)}
        onFocus={(e) => e.currentTarget.select()}
        onBlur={() => setTyped(null)}
        onKeyDown={(e) => {
          if (e.key === "Escape") setTyped(null);
        }}
        spellCheck={false}
        autoComplete="off"
        data-slot="location"
        className={cn(LOCATION, "focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground")}
      />
    </form>
  );
}
