/**
 * Light, dark, or the system's choice. The preference is the only thing stored, in one cookie, so
 * the server renders the right mode for an explicit choice; "system" is resolved before first paint
 * by `THEME_SCRIPT`, which also follows `prefers-color-scheme` while the page is open.
 */
export const THEME_COOKIE = "owlhead-theme";
export const THEME_PREFS = ["light", "dark", "system"] as const;
export type ThemePref = (typeof THEME_PREFS)[number];
export type ThemeMode = "light" | "dark";

const YEAR = 60 * 60 * 24 * 365;

export function isThemePref(value: unknown): value is ThemePref {
  return typeof value === "string" && (THEME_PREFS as readonly string[]).includes(value);
}

export function systemMode(): ThemeMode {
  return typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function resolveMode(pref: ThemePref): ThemeMode {
  switch (pref) {
    case "light":
    case "dark":
      return pref;
    case "system":
      return systemMode();
    default: {
      const unhandled: never = pref;
      throw new Error(`unhandled theme ${String(unhandled)}`);
    }
  }
}

export function applyMode(mode: ThemeMode): void {
  const root = document.documentElement;
  root.dataset.mode = mode;
  root.classList.toggle("dark", mode === "dark");
  root.style.colorScheme = mode;
}

export function readThemePref(): ThemePref {
  const match = document.cookie.match(new RegExp(`(?:^|; )${THEME_COOKIE}=([^;]*)`));
  const value = match ? decodeURIComponent(match[1]) : undefined;
  return isThemePref(value) ? value : "system";
}

export function writeThemePref(pref: ThemePref): void {
  document.cookie = `${THEME_COOKIE}=${pref}; path=/; max-age=${YEAR}; samesite=lax`;
  document.documentElement.dataset.themePref = pref;
  applyMode(resolveMode(pref));
}

/** Runs in `<head>` before the body paints. Kept dependency-free: it is inlined as a string. */
export const THEME_SCRIPT = `(function(){try{var d=document.documentElement,q=window.matchMedia("(prefers-color-scheme: dark)");function p(){var m=document.cookie.match(/(?:^|; )${THEME_COOKIE}=([^;]*)/);var v=m?decodeURIComponent(m[1]):"system";return v==="light"||v==="dark"?v:"system"}function a(){var v=p();d.dataset.themePref=v;var k=v==="system"?(q.matches?"dark":"light"):v;d.dataset.mode=k;d.classList.toggle("dark",k==="dark");d.style.colorScheme=k}a();q.addEventListener("change",function(){if(p()==="system")a()})}catch(e){}})();`;
