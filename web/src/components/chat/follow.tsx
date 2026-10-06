"use client";

import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowDown } from "pixelarticons/react/ArrowDown.js";
import { cn } from "@/lib/utils";

/** Within this distance of the end, the reader is at the latest entry and the log follows new ones. */
const NEAR_END_PX = 48;

/** For this long after a thread opens, it settles at its end at once while fonts and cards still grow it. */
const SETTLE_MS = 600;

export interface Follow {
  /** The element that scrolls. */
  scroller: RefObject<HTMLDivElement | null>;
  /** What grows inside it. */
  content: RefObject<HTMLDivElement | null>;
  /** The reader has scrolled up, away from the latest entry, and the log no longer follows. */
  away: boolean;
  /** Back to the latest entry, following again. */
  jump: () => void;
  /** Follow whatever comes next, wherever the reader is: they have just sent it. */
  pin: () => void;
}

function motion(): ScrollBehavior {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth";
}

function toEnd(el: HTMLElement | null, behavior: ScrollBehavior) {
  if (!el) return;
  if (behavior === "instant" || typeof el.scrollTo !== "function") el.scrollTop = el.scrollHeight;
  else el.scrollTo({ top: el.scrollHeight, behavior });
}

/**
 * A chat log that keeps the latest entry in view while the reader is at it, and leaves them be once
 * they scroll up to read: only scrolling up stops the following, so the log's own smooth scroll, or
 * content growing under it, never does. Reaching the end again resumes it. A new `thread` opens at
 * its end.
 */
export function useFollow(thread: string): Follow {
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const openedAt = useRef(0);
  const [awayIn, setAwayIn] = useState<string | null>(null);

  useLayoutEffect(() => {
    following.current = true;
    openedAt.current = performance.now();
    toEnd(scroller.current, "instant");
  }, [thread]);

  useEffect(() => {
    const el = scroller.current;
    const box = content.current;
    if (!el || !box) return;
    let lastTop = el.scrollTop;
    let height = box.offsetHeight;
    const onScroll = () => {
      const gap = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (gap <= NEAR_END_PX) {
        following.current = true;
        setAwayIn(null);
      } else if (el.scrollTop < lastTop - 1) {
        following.current = false;
        setAwayIn(thread);
      }
      lastTop = el.scrollTop;
    };
    const grown = () => {
      const grew = box.offsetHeight > height;
      height = box.offsetHeight;
      if (following.current && grew) toEnd(el, performance.now() - openedAt.current < SETTLE_MS ? "instant" : motion());
    };
    const resized = () => {
      if (following.current) toEnd(el, "instant");
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    if (typeof ResizeObserver === "undefined") return () => el.removeEventListener("scroll", onScroll);
    const growth = new ResizeObserver(grown);
    const frame = new ResizeObserver(resized);
    growth.observe(box);
    frame.observe(el);
    return () => {
      el.removeEventListener("scroll", onScroll);
      growth.disconnect();
      frame.disconnect();
    };
  }, [thread]);

  const jump = useCallback(() => {
    following.current = true;
    setAwayIn(null);
    toEnd(scroller.current, motion());
  }, []);
  const pin = useCallback(() => {
    following.current = true;
    setAwayIn(null);
  }, []);

  return { scroller, content, away: awayIn === thread, jump, pin };
}

/** Over the composer while the reader is scrolled up: one press back to the latest entry. */
export function JumpToLatest({ away, onJump, className }: { away: boolean; onJump: () => void; className?: string }) {
  if (!away) return null;
  return (
    <button
      type="button"
      data-slot="jump-to-latest"
      onClick={onJump}
      className={cn(
        "press absolute bottom-full left-1/2 z-10 mb-3 inline-flex h-10 -translate-x-1/2 items-center gap-1.5 rounded-full border border-border bg-card pr-4 pl-3 text-sm font-medium text-foreground shadow-md outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring max-lg:h-11",
        className,
      )}
    >
      <ArrowDown aria-hidden className="size-6" />
      Jump to latest
    </button>
  );
}
