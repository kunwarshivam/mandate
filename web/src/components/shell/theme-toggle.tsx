"use client";

import { useSyncExternalStore } from "react";
import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";

/** The only value this app keeps in browser storage. */
export const THEME_KEY = "mandate-theme";

/** Runs before first paint so the page never flashes the wrong theme. */
export const themeScript = `(function(){try{var t=localStorage.getItem("${THEME_KEY}");var d=t?t==="dark":matchMedia("(prefers-color-scheme: dark)").matches;document.documentElement.classList.toggle("dark",d)}catch(e){}})()`;

function subscribe(onChange: () => void) {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
  return () => observer.disconnect();
}

const isDark = () => document.documentElement.classList.contains("dark");

export function ThemeToggle() {
  const dark = useSyncExternalStore(subscribe, isDark, () => false);

  const toggle = () => {
    const next = !isDark();
    document.documentElement.classList.toggle("dark", next);
    window.localStorage.setItem(THEME_KEY, next ? "dark" : "light");
  };

  return (
    <Button variant="ghost" size="icon" className="press size-9" onClick={toggle} aria-label={dark ? "Use light theme" : "Use dark theme"}>
      {dark ? <Sun aria-hidden /> : <Moon aria-hidden />}
    </Button>
  );
}
