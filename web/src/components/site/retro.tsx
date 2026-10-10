import type { ReactNode } from "react";
import { Key } from "pixelarticons/react/Key.js";
import { BrandOwl } from "@/components/brand/brand-owl";
import { cn } from "@/lib/utils";
import { type Guide, LocationField, MenuBar, Toolbar } from "./browser-chrome";
import { MONO, PIXEL, PLAIN_BUTTON, RAISED, SUNKEN, WINDOW_FRAME } from "./letter";
import styles from "./letter.module.css";
import { OpenApp } from "./open-app";
import { StatusText } from "./status-text";
import { TitleBar } from "./title-bar";

export function Blink({ children }: { children: ReactNode }) {
  return <span className={styles.blink}>{children}</span>;
}

/** A dialog window: the title bar and its body. */
export function Window({ title, icon, children, className }: { title: string; icon?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <div className={cn(WINDOW_FRAME, className)} data-slot="window">
      <TitleBar title={title} icon={icon} />
      <div className="p-4 sm:p-5">{children}</div>
    </div>
  );
}

/** The row under the address, where a browser kept its guides. Here they open parts of the page, or its windows on the desktop. */
export const DIRECTORY: Guide[] = [
  { label: "What's New?", href: "#status" },
  { label: "What's Cool?", app: "record" },
  { label: "Handbook", href: "#how" },
  { label: "Questions", app: "questions" },
];

/**
 * The page open in a browser of 1996, inside its desktop window: menus, toolbar, the address, the
 * guide buttons, the page, which scrolls on its own, and a status bar that shows where a link goes.
 * What a browser could do for this page works; what it could not is greyed out, as it was then. The
 * status bar alone is scenery, hidden from assistive technology.
 */
export function Browser({ address, bookmarks, children }: { address: string; bookmarks: { id: string; title: string }[]; children: ReactNode }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-slot="browser">
      <MenuBar bookmarks={bookmarks} directory={DIRECTORY} />

      <div className="shrink-0 border-t border-b border-t-card border-b-foreground/40 px-1.5 py-1.5">
        <div className="flex items-stretch justify-between gap-2">
          <Toolbar />
          <span aria-hidden className={cn(SUNKEN, "grid w-14 shrink-0 place-items-center bg-foreground text-highlight")}>
            <BrandOwl className="size-8" />
          </span>
        </div>

        <div className="mt-1.5 flex items-center gap-2">
          <span aria-hidden className={cn("shrink-0 text-[0.9375rem]", PIXEL)}>
            Location:
          </span>
          <LocationField address={address} />
        </div>

        <nav aria-label="Guides" className="mt-1.5">
          <ul className="grid grid-cols-2 gap-1 sm:flex sm:flex-wrap">
            {DIRECTORY.map((d) => (
              <li key={d.label} className="grid">
                {"href" in d ? (
                  <a href={d.href} className={PLAIN_BUTTON}>
                    {d.label}
                  </a>
                ) : (
                  <OpenApp app={d.app} className={PLAIN_BUTTON}>
                    {d.label}
                  </OpenApp>
                )}
              </li>
            ))}
          </ul>
        </nav>
      </div>

      <div className={cn(SUNKEN, "min-h-0 flex-1 overflow-y-auto overscroll-contain bg-card")} data-scroll-root>
        {children}
      </div>

      <div aria-hidden className={cn("mt-0.5 flex shrink-0 gap-0.5 text-[0.875rem]", PIXEL)}>
        <span className={cn(SUNKEN, "grid w-8 shrink-0 place-items-center")}>
          <Key className="size-6" />
        </span>
        <StatusText origin={address} className={cn(SUNKEN, "min-w-0 flex-1 truncate px-2 py-0.5")} />
        <span className={cn(SUNKEN, "hidden w-40 px-2 py-0.5 sm:block")}>Private beta</span>
      </div>
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

type Badge = { top: string; bottom: string; tone: "sun" | "ink" | "paper" };

export const BADGES: Badge[] = [
  { top: "Private", bottom: "beta", tone: "sun" },
  { top: "Paper", bottom: "first", tone: "ink" },
  { top: "No", bottom: "withdrawals", tone: "paper" },
  { top: "Any", bottom: "browser", tone: "paper" },
  { top: "Y2K", bottom: "ready", tone: "ink" },
];

const TONE: Record<Badge["tone"], string> = {
  sun: "bg-highlight text-highlight-foreground",
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
