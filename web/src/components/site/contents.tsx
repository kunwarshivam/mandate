"use client";

import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";
import { BOLD, LINK, SUNKEN } from "./letter";

type Item = { id: string; title: string };

/** How far down the window a heading must pass before its section counts as the one being read. */
const READING_LINE = 0.3;

function reading(ids: string[]): string | null {
  const bottom = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2;
  if (bottom) return ids.at(-1) ?? null;
  let current: string | null = null;
  for (const id of ids) {
    const top = document.getElementById(id)?.getBoundingClientRect().top;
    if (top !== undefined && top <= window.innerHeight * READING_LINE) current = id;
  }
  return current;
}

/**
 * The contents frame. The section being read is selected, the way a list box of the time drew its
 * selection: ink behind paper type.
 */
export function Contents({ items }: { items: Item[] }) {
  const [current, setCurrent] = useState<string | null>(null);

  useEffect(() => {
    const ids = items.map((i) => i.id);
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setCurrent(reading(ids)));
    };
    update();
    window.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, [items]);

  return (
    <nav aria-labelledby="contents-title" className={cn(SUNKEN, "bg-muted px-4 py-3 lg:sticky lg:top-4")} data-slot="contents">
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
    </nav>
  );
}
