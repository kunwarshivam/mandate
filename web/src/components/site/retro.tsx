import type { ReactNode } from "react";
import { ArrowClockwise, ArrowLeft, ArrowRight, FolderOpen, HandPalm, House, Image as ImageIcon, Key, MagnifyingGlass, Printer } from "@phosphor-icons/react/ssr";
import { OwlheadMark } from "@/components/brand/Logo";
import { cn } from "@/lib/utils";
import { MONO, PIXEL, PLAIN_BUTTON, RAISED, SUNKEN } from "./letter";
import styles from "./letter.module.css";
import { StatusText } from "./status-text";

export function Blink({ children }: { children: ReactNode }) {
  return <span className={styles.blink}>{children}</span>;
}

const WINDOW_BUTTONS = ["_", "□", "×"];

/** A title bar from a 1990s desktop: dark, with the three buttons at the right. */
function TitleBar({ title, icon }: { title: string; icon?: ReactNode }) {
  return (
    <div className={cn("flex h-7 items-center justify-between gap-3 bg-foreground ps-1.5 pe-0.5 text-[0.9375rem] text-card", PIXEL)}>
      <span className="flex min-w-0 items-center gap-1.5">
        {icon}
        <span className="truncate">{title}</span>
      </span>
      <span aria-hidden className="flex shrink-0 gap-0.5">
        {WINDOW_BUTTONS.map((glyph) => (
          <span key={glyph} className={cn(RAISED, "grid size-5 place-items-center bg-muted text-xs leading-none text-foreground")}>
            {glyph}
          </span>
        ))}
      </span>
    </div>
  );
}

/** A dialog window: the title bar and a grey body. */
export function Window({ title, children, className }: { title: string; children: ReactNode; className?: string }) {
  return (
    <div className={cn(RAISED, "bg-muted p-0.5 ring-1 ring-foreground/70", className)} data-slot="window">
      <TitleBar title={title} />
      <div className="p-4 sm:p-5">{children}</div>
    </div>
  );
}

const MENU = ["File", "Edit", "View", "Go", "Bookmarks", "Options", "Directory", "Window", "Help"];

const ICON = "size-5";

/** The toolbar is scenery; Forward is greyed, as it was on any page you'd just opened. */
const TOOLS: { label: string; icon: ReactNode; off?: boolean; wide?: boolean }[] = [
  { label: "Back", icon: <ArrowLeft className={ICON} /> },
  { label: "Forward", icon: <ArrowRight className={ICON} />, off: true },
  { label: "Home", icon: <House className={ICON} /> },
  { label: "Reload", icon: <ArrowClockwise className={ICON} /> },
  { label: "Images", icon: <ImageIcon className={ICON} />, wide: true },
  { label: "Open", icon: <FolderOpen className={ICON} />, wide: true },
  { label: "Print", icon: <Printer className={ICON} />, wide: true },
  { label: "Find", icon: <MagnifyingGlass className={ICON} />, wide: true },
  { label: "Stop", icon: <HandPalm className={ICON} />, wide: true },
];

/** The row under the address, where a browser kept its guides. Here they open parts of the page. */
export const DIRECTORY = [
  { label: "What's New?", href: "#status" },
  { label: "What's Cool?", href: "#record" },
  { label: "Handbook", href: "#how" },
  { label: "Questions", href: "#questions" },
];

/**
 * The page open in a browser of 1996: title bar, menus, toolbar, the address, the guide buttons, and
 * a status bar that shows where a link goes. Only the guide buttons work; the rest is hidden from
 * assistive technology.
 */
export function Browser({ address, children }: { address: string; children: ReactNode }) {
  return (
    <div className={cn(RAISED, "bg-muted p-0.5 ring-1 ring-foreground/70")} data-slot="browser">
      <TitleBar title="Owlhead Home Page" icon={<OwlheadMark title="" className="size-4 shrink-0" />} />

      <div aria-hidden className={cn("flex gap-4 overflow-hidden px-2 py-0.5 text-[0.9375rem] whitespace-nowrap", PIXEL)}>
        {MENU.map((m) => (
          <span key={m}>
            <span className="underline">{m[0]}</span>
            {m.slice(1)}
          </span>
        ))}
      </div>

      <div className="border-t border-b border-t-card border-b-foreground/40 px-1.5 py-1.5">
        <div className="flex items-stretch justify-between gap-2">
          <div aria-hidden className={cn("flex flex-wrap gap-1", PIXEL)}>
            {TOOLS.map((t) => (
              <span key={t.label} className={cn(RAISED, "grid w-[4.25rem] justify-items-center gap-0.5 bg-muted px-1 py-1 text-[0.8125rem] leading-none", t.off && "text-foreground/35", t.wide && "hidden sm:grid")}>
                {t.icon}
                {t.label}
              </span>
            ))}
          </div>
          <span aria-hidden className={cn(SUNKEN, "grid w-14 shrink-0 place-items-center bg-foreground text-highlight")}>
            <OwlheadMark title="" className="size-9" />
          </span>
        </div>

        <div className="mt-1.5 flex items-center gap-2">
          <span aria-hidden className={cn("shrink-0 text-[0.9375rem]", PIXEL)}>
            Location:
          </span>
          <span className={cn(SUNKEN, "min-w-0 flex-1 truncate bg-card px-2 py-0.5 text-lg leading-tight", MONO)}>
            <span className="sr-only">Address: </span>
            {address}
          </span>
        </div>

        <nav aria-label="Guides" className="mt-1.5">
          <ul className="grid grid-cols-2 gap-1 sm:flex sm:flex-wrap">
            {DIRECTORY.map((d) => (
              <li key={d.href} className="grid">
                <a href={d.href} className={PLAIN_BUTTON}>
                  {d.label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </div>

      <div className={cn(SUNKEN, "bg-card")}>{children}</div>

      <div aria-hidden className={cn("mt-0.5 flex gap-0.5 text-[0.875rem]", PIXEL)}>
        <span className={cn(SUNKEN, "grid w-8 shrink-0 place-items-center")}>
          <Key className="size-4" />
        </span>
        <StatusText origin={address} className={cn(SUNKEN, "min-w-0 flex-1 truncate px-2 py-0.5")} />
        <span className={cn(SUNKEN, "hidden w-40 px-2 py-0.5 sm:block")}>Private beta</span>
      </div>
    </div>
  );
}

/** The desktop behind the browser: grey, dithered in a two by two grid the way 16 colours made greys. */
export function Desktop({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cn("relative isolate bg-muted", className)}>
      <svg aria-hidden className="absolute inset-0 -z-10 size-full">
        <defs>
          <pattern id="dither" width="4" height="4" patternUnits="userSpaceOnUse">
            <rect width="1" height="1" className="fill-foreground/15" />
            <rect x="2" y="2" width="1" height="1" className="fill-foreground/15" />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill="url(#dither)" />
      </svg>
      {children}
    </div>
  );
}

/** The yellow and black tape every unfinished page had, drawn as an SVG pattern. */
export function UnderConstruction() {
  return (
    <div className={cn(SUNKEN, "grid w-fit max-w-full bg-card")} data-slot="under-construction">
      <svg aria-hidden viewBox="0 0 240 12" preserveAspectRatio="none" className="block h-3 w-full">
        <defs>
          <pattern id="tape" width="16" height="12" patternUnits="userSpaceOnUse" patternTransform="skewX(-40)">
            <rect width="8" height="12" className="fill-warning" />
            <rect x="8" width="8" height="12" className="fill-foreground" />
          </pattern>
        </defs>
        <rect width="240" height="12" fill="url(#tape)" />
      </svg>
      <p className={cn("px-4 py-1 text-center text-lg tracking-[0.2em] uppercase", MONO)}>Under construction</p>
      <svg aria-hidden viewBox="0 0 240 12" preserveAspectRatio="none" className="block h-3 w-full">
        <rect width="240" height="12" fill="url(#tape)" />
      </svg>
    </div>
  );
}

type Badge = { top: string; bottom: string; tone: "volt" | "ink" | "paper" };

export const BADGES: Badge[] = [
  { top: "Private", bottom: "beta", tone: "volt" },
  { top: "Paper", bottom: "first", tone: "ink" },
  { top: "No", bottom: "withdrawals", tone: "paper" },
  { top: "Any", bottom: "browser", tone: "paper" },
  { top: "Y2K", bottom: "ready", tone: "ink" },
];

const TONE: Record<Badge["tone"], string> = {
  volt: "bg-highlight text-highlight-foreground",
  ink: "bg-foreground text-card",
  paper: "bg-card text-foreground",
};

/** The 88 by 31 buttons that sat along the bottom of every homepage. */
export function Badges({ className }: { className?: string }) {
  return (
    <ul aria-label="Badges" className={cn("flex flex-wrap gap-1.5", className)}>
      {BADGES.map((b) => (
        <li key={b.top + b.bottom} className={cn(RAISED, "grid h-[31px] w-[88px] place-content-center text-center text-[11px] leading-[1.05] tracking-wide uppercase", PIXEL, TONE[b.tone])}>
          <span>{b.top}</span>
          <span>{b.bottom}</span>
        </li>
      ))}
    </ul>
  );
}
