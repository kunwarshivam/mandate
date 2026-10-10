"use client";

import { type KeyboardEvent, type MouseEvent, type PointerEvent, type ReactNode, useEffect, useReducer, useRef, useState, useSyncExternalStore } from "react";
import { createPortal, flushSync } from "react-dom";
import dynamic from "next/dynamic";
import Link from "next/link";
import { BrandOwl } from "@/components/brand/brand-owl";
import { cn } from "@/lib/utils";
import { DISCARDED, Guestbook, Help, Notepad, PictureViewer, RecordViewer, RecycleBin } from "./apps";
import { Assistant } from "./assistant";
import { MENU_ITEM, MONO, PIXEL, RAISED, SUNKEN } from "./letter";
import { MediaPlayer } from "./media-player";
import { OpenAppContext } from "./open-app";
import { BIN, BIN_EMPTY, BOLT, BOOK, FILM, HELP, KEY, LEDGER, MONITOR, NOTE, PICTURE, PixelIcon, type Sprite } from "./pixel-icons";
import { TitleBar, WINDOW_BUTTON } from "./retro";
import { ThemeSwitch } from "./theme-switch";
import { DisplayProperties, Wallpaper } from "./wallpaper";
import { leave, leaving, moving, openFrom, reframe } from "./window-motion";
import { type AppId, TASK } from "./windows";
import type { AmpState } from "./winamp";

/** Webamp is a megabyte of player, so it loads only when someone opens Winamp. */
const Winamp = dynamic(() => import("./winamp"), { ssr: false });


type Win = { open: boolean; min: boolean; max: boolean; x: number; y: number; z: number };

type State = { top: number; wins: Record<AppId, Win> };

type Action =
  | { type: "open"; id: AppId }
  | { type: "close"; id: AppId }
  | { type: "minimize"; id: AppId }
  | { type: "maximize"; id: AppId }
  | { type: "focus"; id: AppId }
  | { type: "move"; id: AppId; x: number; y: number }
  | { type: "reset" };

const CLOSED: Win = { open: false, min: false, max: false, x: 0, y: 0, z: 0 };

const closed = (id: AppId): Win => ({ ...CLOSED, ...APPS[id].offset });

/**
 * A window opened while another is in front sits this far down and right of it, as a 1990s desktop
 * cascaded its windows, so each new one shows the one before it rather than covering it. The home
 * page is the desktop's ground and is never cascaded from.
 */
const CASCADE = 28;

function cascadeFrom(state: State): Pick<Win, "x" | "y"> | null {
  const front = frontmost(state);
  if (front === null || front === "home") return null;
  const w = state.wins[front];
  return { x: w.x + CASCADE, y: w.y + CASCADE };
}

function initial(): State {
  const wins = Object.fromEntries((Object.keys(APPS) as AppId[]).map((id) => [id, closed(id)])) as Record<AppId, Win>;
  return { top: 1, wins: { ...wins, home: { ...wins.home, open: true, z: 1 } } };
}

function reduce(state: State, action: Action): State {
  if (action.type === "reset") return initial();
  const win = state.wins[action.id];
  const set = (patch: Partial<Win>, raise = false): State => ({
    top: raise ? state.top + 1 : state.top,
    wins: { ...state.wins, [action.id]: { ...win, ...patch, ...(raise && { z: state.top + 1 }) } },
  });
  switch (action.type) {
    case "open":
      return set({ open: true, min: false, ...(win.open ? null : cascadeFrom(state)) }, true);
    case "close":
      return set(closed(action.id));
    case "minimize":
      return set({ min: true });
    case "maximize":
      return set({ max: !win.max }, true);
    case "focus":
      return win.z === state.top ? state : set({}, true);
    case "move":
      return set({ x: action.x, y: action.y });
    default: {
      const unhandled: never = action;
      return unhandled;
    }
  }
}

/** The window in front: open, not minimized, and highest. */
function frontmost(state: State): AppId | null {
  let front: AppId | null = null;
  for (const id of Object.keys(state.wins) as AppId[]) {
    const w = state.wins[id];
    if (w.open && !w.min && (front === null || w.z > state.wins[front].z)) front = id;
  }
  return front;
}

type App = { title: string; icon: ReactNode; frame: string; offset: { x: number; y: number } };

/**
 * Each window's size on a wide screen. Every window opens centred on the desktop, the home page
 * clear of the icons on both sides, and the others a little off centre so a second one never hides
 * the first exactly. On a phone every window fills the desktop.
 */
const APPS: Record<AppId, App> = {
  home: {
    title: "Owlhead Home Page",
    icon: <BrandOwl className="size-4" />,
    frame: "sm:inset-y-3 sm:mx-auto sm:w-[min(66rem,calc(100%-15rem))]",
    offset: { x: 0, y: 0 },
  },
  record: {
    title: "The record - Example decision",
    icon: <PixelIcon sprite={LEDGER} className="size-4" />,
    frame: "sm:m-auto sm:h-fit sm:max-h-[calc(100%-2rem)] sm:w-[min(46rem,calc(100%-2rem))]",
    offset: { x: -24, y: 12 },
  },
  questions: {
    title: "Questions - Owlhead Help",
    icon: <PixelIcon sprite={HELP} className="size-4" />,
    frame: "sm:m-auto sm:h-[min(34rem,calc(100%-2rem))] sm:w-[34rem]",
    offset: { x: -72, y: -12 },
  },
  guestbook: {
    title: "guestbook.cgi",
    icon: <PixelIcon sprite={BOOK} className="size-4" />,
    frame: "sm:m-auto sm:h-fit sm:max-h-[calc(100%-2rem)] sm:w-[30rem]",
    offset: { x: 36, y: 24 },
  },
  readme: {
    title: "readme.txt - Notepad",
    icon: <PixelIcon sprite={NOTE} className="size-4" />,
    frame: "sm:m-auto sm:h-[min(27rem,calc(100%-4rem))] sm:w-[27rem]",
    offset: { x: -48, y: -24 },
  },
  owl: {
    title: "owl.jpg - Picture Viewer",
    icon: <PixelIcon sprite={PICTURE} className="size-4" />,
    frame: "sm:m-auto sm:h-[min(38rem,calc(100%-2rem))] sm:w-[23rem]",
    offset: { x: 48, y: 0 },
  },
  display: {
    title: "Display Properties",
    icon: <PixelIcon sprite={MONITOR} className="size-4" />,
    frame: "sm:m-auto sm:h-fit sm:max-h-[calc(100%-2rem)] sm:w-[28rem]",
    offset: { x: 0, y: 24 },
  },
  tour: {
    title: "Tour.mp4 - Media Player",
    icon: <PixelIcon sprite={FILM} className="size-4" />,
    frame: "sm:m-auto sm:h-fit sm:max-h-[calc(100%-2rem)] sm:w-[min(44rem,calc(100%-2rem))]",
    offset: { x: 24, y: -12 },
  },
  bin: {
    title: "Recycle Bin",
    icon: <PixelIcon sprite={BIN} className="size-4" />,
    frame: "sm:m-auto sm:h-[min(25rem,calc(100%-2rem))] sm:w-[34rem]",
    offset: { x: 72, y: -36 },
  },
};

/** `right` icons sit down the desktop's right edge on a wide screen, as a Recycle Bin often did. */
type Shortcut = { id: string; label: string; icon: ReactNode; right?: true } & ({ app: AppId } | { href: string } | { amp: true });

const sprite = (s: Sprite) => <PixelIcon sprite={s} />;

const SHORTCUTS: Shortcut[] = [
  { id: "owlhead", label: "Owlhead", icon: <BrandOwl className="size-8" />, app: "home" },
  { id: "guestbook", label: "Guestbook", icon: sprite(BOOK), app: "guestbook" },
  { id: "record", label: "The record", icon: sprite(LEDGER), app: "record" },
  { id: "questions", label: "Questions", icon: sprite(HELP), app: "questions" },
  { id: "readme", label: "readme.txt", icon: sprite(NOTE), app: "readme" },
  { id: "owl", label: "owl.jpg", icon: sprite(PICTURE), app: "owl" },
  { id: "display", label: "Display", icon: sprite(MONITOR), app: "display" },
  { id: "winamp", label: "Winamp", icon: sprite(BOLT), amp: true },
  { id: "tour", label: "Tour.mp4", icon: sprite(FILM), app: "tour", right: true },
  { id: "bin", label: "Recycle Bin", icon: sprite(BIN), app: "bin", right: true },
  { id: "signin", label: "Sign in", icon: sprite(KEY), href: "/login" },
];

/** The owl assistant says hello this long after the desktop opens, once a visit. */
const HELLO = 9_000;

const WIDE = "(min-width: 40rem)";

/** How much of a dragged window must stay on the desktop, so its title bar can always be caught. */
const GRIP = 96;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

const windowOf = (id: AppId) => document.getElementById(`win-${id}`);

/** Enter and Space click with no pointer behind them, and a keyboard action never animates (`web/DESIGN.md`). */
const fromKeyboard = (e?: MouseEvent<Element>) => e?.detail === 0;

const PRESS = "cursor-pointer active:border-t-foreground/60 active:border-l-foreground/60 active:border-r-card active:border-b-card outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground";

/** The title bar's glyphs, as 8 by 8 pixel drawings. */
const GLYPHS = {
  minimize: [[1, 6, 5, 2]],
  maximize: [
    [0, 0, 8, 2],
    [0, 2, 1, 6],
    [7, 2, 1, 6],
    [0, 7, 8, 1],
  ],
  restore: [
    [2, 0, 6, 1],
    [7, 1, 1, 4],
    [0, 3, 6, 2],
    [0, 5, 1, 3],
    [5, 5, 1, 3],
    [0, 7, 6, 1],
  ],
  close: [
    [0, 0, 2, 1],
    [6, 0, 2, 1],
    [1, 1, 2, 1],
    [5, 1, 2, 1],
    [2, 2, 4, 1],
    [3, 3, 2, 2],
    [2, 5, 4, 1],
    [1, 6, 2, 1],
    [5, 6, 2, 1],
    [0, 7, 2, 1],
    [6, 7, 2, 1],
  ],
} as const;

function Glyph({ name }: { name: keyof typeof GLYPHS }) {
  return (
    <svg aria-hidden viewBox="0 0 8 8" shapeRendering="crispEdges" className="size-2 fill-current">
      {GLYPHS[name].map(([x, y, w, h]) => (
        <rect key={`${x}.${y}`} x={x} y={y} width={w} height={h} />
      ))}
    </svg>
  );
}

type Click = MouseEvent<Element>;

type Controls = { minimize: (id: AppId, e?: Click) => void; maximize: (id: AppId, e?: Click) => void; close: (id: AppId, e?: Click) => void };

function Window({ id, win, layer, front, dispatch, controls, children }: { id: AppId; win: Win; layer: number; front: boolean; dispatch: (a: Action) => void; controls: Controls; children: ReactNode }) {
  const { title, icon, frame } = APPS[id];
  const ref = useRef<HTMLElement>(null);
  const drag = useRef<{ sx: number; sy: number; ox: number; oy: number; minX: number; maxX: number; minY: number; maxY: number } | null>(null);

  const grab = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || win.max || (e.target as Element).closest("button")) return;
    if (!window.matchMedia(WIDE).matches) return;
    const el = ref.current;
    const desk = el?.parentElement;
    if (!el || !desk) return;
    const r = el.getBoundingClientRect();
    const d = desk.getBoundingClientRect();
    drag.current = {
      sx: e.clientX,
      sy: e.clientY,
      ox: win.x,
      oy: win.y,
      minX: win.x + d.left - r.right + GRIP,
      maxX: win.x + d.right - r.left - GRIP,
      minY: win.y + d.top - r.top,
      maxY: win.y + d.bottom - r.top - 28,
    };
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };

  const pull = (e: PointerEvent<HTMLDivElement>) => {
    const g = drag.current;
    if (!g) return;
    dispatch({ type: "move", id, x: clamp(g.ox + e.clientX - g.sx, g.minX, g.maxX), y: clamp(g.oy + e.clientY - g.sy, g.minY, g.maxY) });
  };

  const drop = () => {
    drag.current = null;
  };

  return (
    <section
      ref={ref}
      id={`win-${id}`}
      aria-label={title}
      tabIndex={-1}
      hidden={!win.open || win.min}
      data-slot="os-window"
      data-window={id}
      data-front={front || undefined}
      data-maximized={win.max || undefined}
      onPointerDownCapture={() => dispatch({ type: "focus", id })}
      onFocusCapture={() => dispatch({ type: "focus", id })}
      className={cn(RAISED, "absolute inset-0 flex origin-top-left flex-col bg-muted p-0.5 ring-1 ring-foreground/70 outline-none max-sm:translate-none!", !win.max && frame)}
      style={{ zIndex: layer, translate: win.max ? undefined : `${win.x}px ${win.y}px` }}
    >
      <TitleBar
        title={title}
        icon={icon}
        inactive={!front}
        onPointerDown={grab}
        onPointerMove={pull}
        onPointerUp={drop}
        onPointerCancel={drop}
        onDoubleClick={(e) => {
          if (!(e.target as Element).closest("button")) controls.maximize(id, e);
        }}
        className={cn("select-none", !win.max && "sm:cursor-grab sm:touch-none sm:active:cursor-grabbing")}
        controls={
          <span className="flex shrink-0 gap-0.5">
            {/* Pointer affordances like the icons: out of the tab cycle, so the window's content
                comes first in the keyboard order (`e2e/landing.spec.ts`). */}
            <button type="button" tabIndex={-1} aria-label={`Minimize ${title}`} onClick={(e) => controls.minimize(id, e)} className={cn(WINDOW_BUTTON, PRESS)}>
              <Glyph name="minimize" />
            </button>
            <button type="button" tabIndex={-1} aria-label={`${win.max ? "Restore" : "Maximize"} ${title}`} onClick={(e) => controls.maximize(id, e)} className={cn(WINDOW_BUTTON, PRESS, "max-sm:hidden")}>
              <Glyph name={win.max ? "restore" : "maximize"} />
            </button>
            <button type="button" tabIndex={-1} aria-label={`Close ${title}`} onClick={(e) => controls.close(id, e)} className={cn(WINDOW_BUTTON, PRESS, "ms-0.5")}>
              <Glyph name="close" />
            </button>
          </span>
        }
      />
      <div className="flex min-h-0 flex-1 flex-col">{children}</div>
    </section>
  );
}

function subscribeClock(onChange: () => void): () => void {
  const t = setInterval(onChange, 10_000);
  return () => clearInterval(t);
}

const now = () => new Date().toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

/** The tray's clock. It shows the visitor's own time, so it waits for the browser before it draws. */
function Clock() {
  const time = useSyncExternalStore(subscribeClock, now, () => "");
  return (
    <span aria-hidden className="w-[4.25rem] text-center text-[0.875rem] sm:w-[4.75rem] sm:text-[0.9375rem]" data-slot="clock">
      {time}
    </span>
  );
}

/** Up and Down move through a menu's items, as a menu of the time did. */
function arrowKeys(e: KeyboardEvent<HTMLElement>) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const items = [...e.currentTarget.querySelectorAll<HTMLElement>("[role=menuitem]")];
  const at = items.indexOf(document.activeElement as HTMLElement);
  items[(at + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
}

function ShortcutItem({ s, onOpen, role, className, tabIndex, children }: { s: Shortcut; onOpen: (s: Shortcut, e: Click) => void; role?: "menuitem"; className?: string; tabIndex?: number; children: ReactNode }) {
  if ("href" in s)
    return (
      <Link href={s.href} role={role} tabIndex={tabIndex} className={className}>
        {children}
      </Link>
    );
  return (
    <button type="button" role={role} tabIndex={tabIndex} onClick={(e) => onOpen(s, e)} className={className}>
      {children}
    </button>
  );
}

/**
 * The landing page as a desktop from the late 1990s (DEC-218): a painting from the Met for wallpaper,
 * icons that open windows, and windows that drag by their title bars, come to the front when touched,
 * and minimize to the taskbar, fill the desktop, or close. The home page is always in the document,
 * so its headings and landmarks are there to read even while its window is hidden.
 */
export function Desktop({ home }: { home: ReactNode }) {
  const [state, dispatch] = useReducer(reduce, undefined, initial);
  const [selected, setSelected] = useState<string | null>(null);
  const [start, setStart] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [off, setOff] = useState(false);
  const [amp, setAmp] = useState<AmpState>("off");
  const [ampLoaded, setAmpLoaded] = useState(false);
  const [ampAnchor, setAmpAnchor] = useState<HTMLElement | null>(null);
  const [bin, setBin] = useState(DISCARDED);
  const [helper, setHelper] = useState(false);
  const front = frontmost(state);
  const startRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!start && !menu) return;
    const shut = (e: Event) => {
      if (e instanceof globalThis.KeyboardEvent ? e.key === "Escape" : !(e.target instanceof Element && e.target.closest("[data-slot=start], [data-slot=desktop-menu]"))) {
        setStart(false);
        setMenu(null);
      }
    };
    document.addEventListener("keydown", shut);
    document.addEventListener("pointerdown", shut);
    return () => {
      document.removeEventListener("keydown", shut);
      document.removeEventListener("pointerdown", shut);
    };
  }, [start, menu]);

  useEffect(() => {
    if (start) startRef.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
  }, [start]);

  useEffect(() => {
    if (!window.matchMedia(WIDE).matches) return;
    const t = setTimeout(() => setHelper(true), HELLO);
    return () => clearTimeout(t);
  }, []);

  /**
   * Opens a window, zooming it out of the control that was clicked, or out of its desktop icon when
   * nothing was. A window opened from the keyboard just appears; one caught mid-close turns around.
   */
  const openApp = (id: AppId, e?: Click) => {
    const el = windowOf(id);
    const w = state.wins[id];
    const from = (e?.currentTarget ?? document.querySelector(`[data-slot=desktop] li[data-app="${id}"]`))?.getBoundingClientRect() ?? null;
    const turning = moving(el);
    flushSync(() => dispatch({ type: "open", id }));
    setStart(false);
    setMenu(null);
    if (el && (turning || ((!w.open || w.min) && !fromKeyboard(e)))) openFrom(el, from);
    requestAnimationFrame(() => el?.focus({ preventScroll: true }));
  };

  const controls: Controls = {
    minimize: (id, e) => {
      const el = windowOf(id);
      const to = document.querySelector(`[data-task="${id}"]`)?.getBoundingClientRect() ?? null;
      if (!el || fromKeyboard(e)) dispatch({ type: "minimize", id });
      else leave(el, to, () => flushSync(() => dispatch({ type: "minimize", id })));
    },
    maximize: (id, e) => {
      const el = windowOf(id);
      if (!el || fromKeyboard(e)) dispatch({ type: "maximize", id });
      else reframe(el, () => flushSync(() => dispatch({ type: "maximize", id })));
    },
    close: (id, e) => {
      const el = windowOf(id);
      if (!el || fromKeyboard(e)) dispatch({ type: "close", id });
      else leave(el, null, () => flushSync(() => dispatch({ type: "close", id })));
    },
  };

  const launch = (s: Shortcut, e?: Click) => {
    setSelected(s.id);
    setStart(false);
    if ("app" in s) openApp(s.app, e);
    if ("amp" in s) {
      setAmpLoaded(true);
      setAmp("open");
      setHelper(false);
    }
  };

  const bodies: Record<AppId, ReactNode> = {
    home,
    record: <RecordViewer />,
    questions: <Help />,
    guestbook: <Guestbook />,
    readme: <Notepad />,
    owl: <PictureViewer />,
    display: <DisplayProperties onDone={() => controls.close("display")} />,
    tour: <MediaPlayer />,
    bin: <RecycleBin items={bin} onEmpty={() => setBin([])} />,
  };

  const iconFor = (s: Shortcut) => (s.id === "bin" && bin.length === 0 ? sprite(BIN_EMPTY) : s.icon);

  const icons = (list: Shortcut[]) =>
    list.map((s) => {
      const on = selected === s.id;
      return (
        <li key={s.id} className="grid justify-items-center" data-app={"app" in s ? s.app : undefined}>
          {/* Desktop icons are pointer affordances, out of the tab cycle as on the real desktop: the
              keyboard reaches the apps through the Start menu, and the page's content comes first
              (`e2e/landing.spec.ts`: skip link, then the guide links). */}
          <ShortcutItem s={s} onOpen={launch} tabIndex={-1} className="group grid w-24 cursor-pointer content-start justify-items-center gap-1 p-1 outline-none">
            <span className={cn("grid size-8 place-items-center", on && "opacity-80")}>{iconFor(s)}</span>
            <span
              className={cn(
                "px-1 text-center text-[0.875rem] leading-tight group-focus-visible:outline-1 group-focus-visible:outline-dotted group-focus-visible:outline-offset-1 group-focus-visible:outline-card",
                PIXEL,
                on ? "bg-highlight text-highlight-foreground" : "bg-foreground text-card",
              )}
            >
              {s.label}
            </span>
          </ShortcutItem>
        </li>
      );
    });

  const open = (Object.keys(APPS) as AppId[]).filter((id) => state.wins[id].open);
  const layers = (Object.keys(APPS) as AppId[]).toSorted((a, b) => state.wins[a].z - state.wins[b].z);

  return (
    <div className={cn("flex h-dvh flex-col overflow-hidden", PIXEL)} data-slot="desktop">
      <div
        className="relative isolate min-h-0 flex-1 overflow-hidden"
        onPointerDown={(e) => {
          if (!(e.target as Element).closest("button, a, [data-slot=os-window]")) setSelected(null);
        }}
        onContextMenu={(e) => {
          if ((e.target as Element).closest("button, a, [data-slot=os-window]")) return;
          e.preventDefault();
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: Math.min(e.clientX - r.left, r.width - 200), y: Math.min(e.clientY - r.top, r.height - 90) });
        }}
      >
        <Wallpaper />
        <div ref={setAmpAnchor} aria-hidden className="pointer-events-none absolute inset-y-0 right-4 w-[275px] sm:right-[8rem] max-sm:inset-x-0 max-sm:mx-auto" />
        {ampLoaded && ampAnchor && <Winamp anchor={ampAnchor} state={amp} onState={setAmp} />}

        <ul aria-label="Desktop" className="grid content-start gap-1 p-2 max-sm:grid-cols-4 sm:h-full sm:grid-flow-col sm:auto-cols-[6.5rem] sm:grid-rows-[repeat(auto-fill,5.25rem)]" data-slot="desktop-icons">
          {icons(SHORTCUTS.filter((s) => !s.right))}
        </ul>
        <ul aria-label="Desktop, right" className="grid content-start gap-1 px-2 max-sm:grid-cols-4 sm:absolute sm:inset-y-0 sm:right-0 sm:w-[7.5rem] sm:py-2" data-slot="desktop-icons-right">
          {icons(SHORTCUTS.filter((s) => s.right))}
        </ul>

        <OpenAppContext value={openApp}>
          {(Object.keys(APPS) as AppId[]).map((id) => (
            <Window key={id} id={id} win={state.wins[id]} layer={layers.indexOf(id) + 1} front={front === id} dispatch={dispatch} controls={controls}>
              {bodies[id]}
            </Window>
          ))}
        </OpenAppContext>

        {menu && (
          <div role="menu" aria-label="Desktop" data-slot="desktop-menu" onKeyDown={arrowKeys} className={cn(RAISED, "absolute z-[60] w-48 bg-muted py-1 ring-1 ring-foreground/70")} style={{ left: menu.x, top: menu.y }}>
            <button type="button" role="menuitem" autoFocus onClick={(e) => openApp("home", e)} className={MENU_ITEM}>
              Open Owlhead
            </button>
            <button type="button" role="menuitem" onClick={(e) => openApp("display", e)} className={MENU_ITEM}>
              Change wallpaper…
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setMenu(null);
                setHelper(true);
              }}
              className={MENU_ITEM}
            >
              Ask the owl…
            </button>
          </div>
        )}

        <Assistant
          shown={helper}
          onClose={() => setHelper(false)}
          onTour={() => {
            setHelper(false);
            openApp("tour");
          }}
        />
      </div>

      <div className={cn("relative z-[70] flex h-10 shrink-0 items-center gap-1 border-t-2 border-t-card bg-muted px-1", PIXEL)} data-slot="taskbar">
        <div data-slot="start" className="contents">
          <button
            type="button"
            aria-haspopup="menu"
            aria-expanded={start}
            aria-controls="start-menu"
            onClick={() => setStart((v) => !v)}
            className={cn(start ? SUNKEN : RAISED, "flex h-8 shrink-0 cursor-pointer items-center gap-1.5 bg-muted px-2 text-[0.9375rem] outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground")}
          >
            <BrandOwl className="size-4" />
            Start
          </button>
          {start && (
            <div ref={startRef} id="start-menu" role="menu" aria-label="Start" onKeyDown={arrowKeys} className={cn(RAISED, "absolute bottom-10 left-1 flex bg-muted ring-1 ring-foreground/70")}>
              <span aria-hidden className={cn("flex w-8 items-end justify-center bg-foreground pb-3 text-lg text-card [writing-mode:vertical-rl]", PIXEL)}>
                <span className="rotate-180">
                  Owlhead <span className="text-highlight">98</span>
                </span>
              </span>
              <ul className="grid min-w-56 py-1">
                {[{ id: "home", label: "Owlhead Home Page", icon: <BrandOwl className="size-6" />, app: "home" as const }, ...SHORTCUTS.slice(1)].map((s) => (
                  <li key={s.id} className={cn(s.id === "signin" && "mt-1 border-t border-t-foreground/40 pt-1")}>
                    <ShortcutItem s={s} onOpen={launch} role="menuitem" className={MENU_ITEM}>
                      <span className="grid size-6 place-items-center [&>svg]:size-6">{iconFor(s)}</span>
                      {s.label}
                    </ShortcutItem>
                  </li>
                ))}
                <li>
                  <button
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      setStart(false);
                      setOff(true);
                    }}
                    className={MENU_ITEM}
                  >
                    <span aria-hidden className="grid size-6 place-items-center">
                      <Glyph name="close" />
                    </span>
                    Shut down…
                  </button>
                </li>
              </ul>
            </div>
          )}
        </div>

        <span aria-hidden className="mx-0.5 h-7 border-r border-l border-r-card border-l-foreground/40" />

        <ul aria-label="Open windows" className="flex min-w-0 flex-1 gap-1">
          {open.map((id) => (
            <li key={id} className="min-w-0 max-w-44 flex-1">
              <button
                type="button"
                aria-pressed={front === id}
                data-task={id}
                onClick={(e) => (front === id && !leaving(windowOf(id)) ? controls.minimize(id, e) : openApp(id, e))}
                className={cn(front === id ? "bg-card" : "bg-muted", front === id ? SUNKEN : RAISED, "flex h-8 w-full min-w-0 cursor-pointer items-center gap-1.5 px-1.5 text-[0.875rem] outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground")}
              >
                {APPS[id].icon}
                <span className="truncate">{TASK[id]}</span>
              </button>
            </li>
          ))}
          {amp !== "off" && (
            <li className="min-w-0 max-w-44 flex-1">
              <button
                type="button"
                aria-pressed={amp === "open"}
                onClick={() => setAmp(amp === "open" ? "minimized" : "open")}
                className={cn(amp === "open" ? cn(SUNKEN, "bg-card") : cn(RAISED, "bg-muted"), "flex h-8 w-full min-w-0 cursor-pointer items-center gap-1.5 px-1.5 text-[0.875rem] outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:-outline-offset-4 focus-visible:outline-foreground")}
              >
                <PixelIcon sprite={BOLT} className="size-4" />
                <span className="truncate">Winamp</span>
              </button>
            </li>
          )}
        </ul>

        <div className={cn(SUNKEN, "flex h-8 shrink-0 items-center gap-1 px-0.5")} data-slot="tray">
          <ThemeSwitch />
          <Clock />
        </div>
      </div>

      {off &&
        createPortal(
          <button
            type="button"
            autoFocus
            onClick={() => {
              setOff(false);
              dispatch({ type: "reset" });
            }}
            className={cn(MONO, "fixed inset-0 z-[100] grid cursor-pointer place-content-center gap-4 bg-ink p-6 text-center text-[clamp(1.75rem,4.5vw,3rem)] leading-tight text-highlight outline-none")}
            data-slot="shut-down"
          >
            <span>
              It&apos;s now safe to turn off
              <br />
              your computer.
            </span>
            <span className="text-xl text-ink-foreground">Click anywhere to start Owlhead again.</span>
          </button>,
          document.body,
        )}
    </div>
  );
}
