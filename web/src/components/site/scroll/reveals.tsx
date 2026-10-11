"use client";

import { useEffect } from "react";

/**
 * Marks the long page as moving, and shows each `[data-reveal]` piece once part of it is in view, so
 * it settles on a spring (`scroll.module.css`). With motion reduced, or without an observer, it does
 * nothing and every piece stays where it rests, as the server drew it.
 */
export function Reveals() {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches || !("IntersectionObserver" in window)) return;
    const page = document.querySelector<HTMLElement>("[data-slot=long-page]");
    if (!page) return;
    const seen = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          (entry.target as HTMLElement).dataset.shown = "";
          seen.unobserve(entry.target);
        }
      },
      { threshold: 0.15, rootMargin: "0px 0px -6% 0px" },
    );
    for (const piece of page.querySelectorAll<HTMLElement>("[data-reveal]")) seen.observe(piece);
    page.dataset.motion = "on";
    return () => {
      seen.disconnect();
      delete page.dataset.motion;
    };
  }, []);
  return null;
}
