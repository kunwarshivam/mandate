"use client";

import { type ReactNode, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

/** Where the rail sticks, `lg:top-24`: the 4rem header and a 2rem gap. The two must change together. */
const STICKY_TOP_PX = 96;
const STICKY_BOTTOM_PX = 16;

/**
 * The narrow column beside a page's story. On desktop it stays in view as the page scrolls, but only
 * while it fits in the window: a sticky rail taller than the window would hide its own end.
 */
export function SideRail({ children, className }: { children: ReactNode; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [fits, setFits] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const check = () => setFits(el.offsetHeight + STICKY_TOP_PX + STICKY_BOTTOM_PX <= window.innerHeight);
    const observer = new ResizeObserver(check);
    observer.observe(el);
    window.addEventListener("resize", check);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", check);
    };
  }, []);

  return (
    <div
      ref={ref}
      data-layout="rail"
      data-sticky={fits ? "" : undefined}
      className={cn("grid min-w-0 content-start gap-(--section-gap) lg:self-start lg:data-[sticky]:sticky lg:data-[sticky]:top-24", className)}
    >
      {children}
    </div>
  );
}
