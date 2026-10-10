"use client";

import { useEffect } from "react";

/** How far outside the window a picture starts moving again, so it is already moving as it scrolls in. */
const AHEAD = "200px 0px";

/**
 * Pauses the long page's ambient loops, the sea's waves, foam, glints and gulls and the lock screen's
 * stars, while their picture is out of view, so a page left open spends nothing on what nobody sees
 * (`scroll.module.css`). Each picture marked `data-ambient` is set to `playing` or `paused`. With
 * motion reduced the loops never run, and without an observer they always do.
 */
export function Ambient() {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches || !("IntersectionObserver" in window)) return;
    const page = document.querySelector<HTMLElement>("[data-slot=long-page]");
    if (!page) return;
    const pictures = [...page.querySelectorAll<HTMLElement>("[data-ambient]")];
    const seen = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) (entry.target as HTMLElement).dataset.ambient = entry.isIntersecting ? "playing" : "paused";
      },
      { rootMargin: AHEAD },
    );
    for (const picture of pictures) seen.observe(picture);
    return () => {
      seen.disconnect();
      for (const picture of pictures) picture.dataset.ambient = "";
    };
  }, []);
  return null;
}
