"use client";

import { type MouseEvent, useEffect, useRef } from "react";
import { cn } from "@/lib/utils";
import { INTRO_ID } from "./parts";
import styles from "./scroll.module.css";

/** Above the Windows taskbar, or the bottom edge on the Mac, when no window's status bar is under the middle. */
const FALLBACK_BOTTOM = 64;
/** Frames in a row with the cue where it already is before it stops measuring until something moves again. */
const QUIET_FRAMES = 30;
const STATUS = "[data-slot=status-text]";

/** The event the cue sends, bubbling, each time it moves, so the owl peeking over it follows. */
export const CUE_MOVED = "cue-moved";

const holdsStatus = (node: Node | EventTarget | null) => node instanceof Element && (node.matches(STATUS) || node.querySelector(STATUS) !== null);

/**
 * The cue that there is more below the desktop (DEC-907): a pill in the middle of the screen, on the
 * row of the browser window's status bar when that bar runs under the middle with room for it, the
 * one place no layout puts anything, followed as the window boots, moves or closes. It measures only
 * while something may have moved it (a window holding the status bar changing or animating, the
 * desktop resizing, fonts loading, or the first screen scrolling) and stops once it has stayed put.
 * The page rising over the desktop covers it, and with motion allowed it fades as it goes.
 */
export function ScrollCue() {
  const ref = useRef<HTMLAnchorElement>(null);

  useEffect(() => {
    const cue = ref.current;
    const stage = cue?.parentElement;
    if (!cue || !stage) return;
    let raf = 0;
    let quiet = 0;
    let placed = "";
    const place = () => {
      raf = 0;
      if (window.scrollY >= window.innerHeight) return;
      const frame = stage.getBoundingClientRect();
      const middle = frame.left + frame.width / 2;
      const half = cue.offsetWidth / 2;
      const status = [...stage.querySelectorAll<HTMLElement>(STATUS)]
        .map((s) => s.getBoundingClientRect())
        .find((r) => r.width > 0 && r.height > 0 && r.left + half <= middle && middle <= r.right - half);
      const left = `${frame.width / 2}px`;
      const top = `${status ? status.top + status.height / 2 - frame.top : frame.height - FALLBACK_BOTTOM}px`;
      const at = status ? "status" : "edge";
      const next = `${left} ${top} ${at}`;
      if (next === placed) quiet++;
      else {
        placed = next;
        quiet = 0;
        cue.style.left = left;
        cue.style.top = top;
        cue.dataset.at = at;
        cue.dispatchEvent(new Event(CUE_MOVED, { bubbles: true }));
      }
      if (quiet < QUIET_FRAMES) raf = window.requestAnimationFrame(place);
    };
    const follow = () => {
      quiet = 0;
      if (!raf) raf = window.requestAnimationFrame(place);
    };
    const onScroll = () => {
      const covered = window.scrollY >= window.innerHeight;
      const ambient = covered ? "paused" : "playing";
      if (cue.dataset.ambient !== ambient) cue.dataset.ambient = ambient;
      if (!covered) follow();
    };
    const outside = (target: Node | EventTarget | null) => !(target instanceof Node && cue.contains(target));
    const onMotion = (e: Event) => {
      if (outside(e.target) && holdsStatus(e.target)) follow();
    };
    const changes = new MutationObserver((records) => {
      const moved = records.some(
        (r) => outside(r.target) && (holdsStatus(r.target) || r.target.parentElement?.closest(STATUS) || [...r.addedNodes, ...r.removedNodes].some(holdsStatus)),
      );
      if (moved) follow();
    });
    const resizes = new ResizeObserver(follow);
    const motions = ["animationstart", "animationend", "transitionstart", "transitionend"] as const;
    place();
    onScroll();
    changes.observe(stage, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["style", "class", "hidden"] });
    resizes.observe(stage);
    for (const m of motions) stage.addEventListener(m, onMotion);
    window.addEventListener("resize", follow);
    window.addEventListener("scroll", onScroll, { passive: true });
    document.fonts?.addEventListener("loadingdone", follow);
    return () => {
      window.cancelAnimationFrame(raf);
      changes.disconnect();
      resizes.disconnect();
      for (const m of motions) stage.removeEventListener(m, onMotion);
      window.removeEventListener("resize", follow);
      window.removeEventListener("scroll", onScroll);
      document.fonts?.removeEventListener("loadingdone", follow);
    };
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
