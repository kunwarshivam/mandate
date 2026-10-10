"use client";

import { useSyncExternalStore } from "react";
import { Moon } from "pixelarticons/react/Moon.js";
import { type ThemeMode, writeThemePref } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { PIXEL, RAISED, SUNKEN } from "./letter";

function subscribe(onChange: () => void): () => void {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-mode"] });
  return () => observer.disconnect();
}

const current = (): ThemeMode => (document.documentElement.dataset.mode === "dark" ? "dark" : "light");

/** The theme in force, or `null` before the browser has said; the Mac's Special menu checks it too. */
export function useThemeMode(): ThemeMode | null {
  return useSyncExternalStore(subscribe, current, () => null);
}

/**
 * Dark mode, in the taskbar's tray: pressed in while it is on, as a toggle of 1996 was. It saves the
 * same choice as the app's theme menu, so the two always agree.
 */
export function ThemeSwitch() {
  const mode = useThemeMode();
  const dark = mode === "dark";
  return (
    <button
      type="button"
      data-slot="theme-switch"
      aria-pressed={mode === null ? undefined : dark}
      onClick={() => writeThemePref(dark ? "light" : "dark")}
      className={cn(
        dark ? SUNKEN : RAISED,
        "flex h-7 cursor-pointer items-center gap-1 bg-muted px-1.5 text-[0.875rem] leading-none outline-none focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-foreground",
        dark && "bg-card",
        PIXEL,
      )}
    >
      <Moon aria-hidden className="size-6" />
      Dark
    </button>
  );
}
