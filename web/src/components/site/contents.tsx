"use client";

import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import type { AppId } from "./desktop";
import { BOLD, LINK, SUNKEN } from "./letter";
import { OpenApp } from "./open-app";

type Item = { id: string; title: string };

type Opens = { app: AppId; title: string };

/** How far down the page's window a heading must pass before its section counts as the one being read. */
const READING_LINE = 0.3;

/** The page scrolls inside its browser window, so "being read" is measured against that pane. */
function reading(ids: string[], pane: HTMLElement): string | null {
  const bottom = pane.scrollTop + pane.clientHeight >= pane.scrollHeight - 2;
  if (bottom) return ids.at(-1) ?? null;
  const line = pane.getBoundingClientRect().top + pane.clientHeight * READING_LINE;
  let current: string | null = null;
  for (const id of ids) {
    const top = document.getElementById(id)?.getBoundingClientRect().top;
    if (top !== undefined && top <= line) current = id;
  }
  return current;
}

/**
 * The contents frame. The section being read is selected, the way a list box of the time drew its
 * selection: ink behind paper type. Under the sections, the parts of the site that open in their own
 * window on the desktop.
 */
export function Contents({ items, windows }: { items: Item[]; windows: Opens[] }) {
  const [current, setCurrent] = useState<string | null>(null);
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    const pane = ref.current?.closest<HTMLElement>("[data-scroll-root]");
    if (!pane) return;
    const ids = items.map((i) => i.id);
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setCurrent(reading(ids, pane)));
    };
    update();
    pane.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      cancelAnimationFrame(frame);
      pane.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, [items]);

  return (
    <nav ref={ref} aria-labelledby="contents-title" className={cn(SUNKEN, "bg-muted px-4 py-3 lg:sticky lg:top-4")} data-slot="contents">
      <h2 id="contents-title" className={cn(BOLD, "pb-1")}>
        Contents
      </h2>
      <ol className="grid list-decimal gap-0.5 ps-6 text-[1.0625rem]">
        {items.map((c) => {
          const here = c.id === current;
          return (
            <li key={c.id}>
              <a href={`#${c.id}`} aria-current={here ? "location" : undefined} className={cn(LINK, "px-0.5", here && "bg-foreground text-card no-underline")}>
                {c.title}
              </a>
            </li>
          );
        })}
      </ol>
      <h3 className={cn(BOLD, "pt-3 pb-1")}>On the desktop</h3>
      <ul className="grid list-disc gap-0.5 ps-6 text-[1.0625rem]">
        {windows.map((w) => (
          <li key={w.app}>
            <OpenApp app={w.app} className={cn(LINK, "cursor-pointer px-0.5 text-start")}>
              {w.title}
            </OpenApp>
          </li>
        ))}
      </ul>
    </nav>
  );
}
