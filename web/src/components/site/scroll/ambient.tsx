"use client";

import { useEffect } from "react";

/** How far outside the window a picture starts moving again, so it is already moving as it scrolls in. */
const AHEAD_PX = 200;
const AHEAD = `${AHEAD_PX}px 0px`;

/**
 * The desktop and the documents of the apps in its browser window, which keep their own loops. A
 * frame part way through loading has a document with no root yet, whatever its type says.
 */
function desktopRoots(stage: HTMLElement): HTMLElement[] {
  const apps = [...stage.querySelectorAll("iframe")].flatMap((frame) => frame.contentDocument?.documentElement ?? []);
  return [stage, ...apps];
}

/**
 * Pauses what nobody can see, so a page left open spends nothing on it (`scroll.module.css`). Each
 * picture marked `data-ambient`, the sea and the lock screen's night, is set to `playing` or `paused`
 * as it comes into view or leaves, and its loops hold still while paused. The desktop under the page
 * is paused once the page has risen `AHEAD_PX` past covering it, with the app in its browser window
 * (`globals.css`), and plays again before it shows. With motion reduced the pictures never move, and
 * without an observer they always do.
 */
export function Ambient() {
  useEffect(() => {
    const stage = document.querySelector<HTMLElement>("[data-slot=desktop-stage]");
    if (!stage) return;
    const cover = () => {
      const state = window.scrollY >= window.innerHeight + AHEAD_PX ? "paused" : "playing";
      for (const root of desktopRoots(stage)) if (root.dataset.ambient !== state) root.dataset.ambient = state;
    };
    cover();
    window.addEventListener("scroll", cover, { passive: true });
    window.addEventListener("resize", cover);
    stage.addEventListener("load", cover, true);
    return () => {
      window.removeEventListener("scroll", cover);
      window.removeEventListener("resize", cover);
      stage.removeEventListener("load", cover, true);
      for (const root of desktopRoots(stage)) delete root.dataset.ambient;
    };
  }, []);

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
