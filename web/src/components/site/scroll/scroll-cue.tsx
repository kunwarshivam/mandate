"use client";

import { type MouseEvent, useEffect, useRef } from "react";
import { cn } from "@/lib/utils";
import { INTRO_ID } from "./parts";
import styles from "./scroll.module.css";

/** Above the Windows taskbar, or the bottom edge on the Mac, when no window's status bar is under the middle. */
const FALLBACK_BOTTOM = 64;

/**
 * The cue that there is more below the desktop (DEC-907): a pill in the middle of the screen, on the
 * row of the browser window's status bar when that bar runs under the middle with room for it, the
 * one place no layout puts anything, followed as the window boots, moves or closes. The page rising
 * over the desktop covers it, and with motion allowed it fades as it goes.
 */
export function ScrollCue() {
  const ref = useRef<HTMLAnchorElement>(null);

  useEffect(() => {
    const cue = ref.current;
    const stage = cue?.parentElement;
    if (!cue || !stage) return;
    let raf = 0;
    const place = () => {
      if (window.scrollY < window.innerHeight) {
        const frame = stage.getBoundingClientRect();
        const middle = frame.left + frame.width / 2;
        const half = cue.offsetWidth / 2;
        const status = [...stage.querySelectorAll<HTMLElement>("[data-slot=status-text]")]
          .map((s) => s.getBoundingClientRect())
          .find((r) => r.width > 0 && r.height > 0 && r.left + half <= middle && middle <= r.right - half);
        cue.style.left = `${frame.width / 2}px`;
        cue.style.top = `${status ? status.top + status.height / 2 - frame.top : frame.height - FALLBACK_BOTTOM}px`;
        cue.dataset.at = status ? "status" : "edge";
      }
      raf = window.requestAnimationFrame(place);
    };
    place();
    return () => window.cancelAnimationFrame(raf);
  }, []);

  const down = (e: MouseEvent<HTMLAnchorElement>) => {
    const intro = document.getElementById(INTRO_ID);
    if (!intro) return;
    e.preventDefault();
    intro.scrollIntoView({ behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "start" });
  };

  return (
    <a
      ref={ref}
      href={`#${INTRO_ID}`}
      onClick={down}
      className={cn(
        styles.cue,
        "inline-flex h-7 items-center gap-1.5 rounded-full bg-highlight px-3 text-[0.8125rem] font-semibold whitespace-nowrap text-highlight-foreground ring-1 ring-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring",
      )}
      data-slot="scroll-cue"
    >
      Scroll
      <svg aria-hidden viewBox="0 0 12 12" className="size-3" data-slot="cue-arrow">
        <path d="M6 1.5v8M2.5 6.5 6 10l3.5-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    </a>
  );
}
