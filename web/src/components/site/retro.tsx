import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { MONO, PIXEL, RAISED, SUNKEN, WINDOW_FRAME } from "./letter";
import styles from "./letter.module.css";
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
