/**
 * Which 1990s desktop the signed-out pages wear: Windows 98 or the Mac's System 7. The choice is kept
 * in one cookie, so the server draws the chosen desktop and nothing swaps after the page paints.
 */
export const DESKTOP_COOKIE = "owlhead-desktop";
export const DESKTOP_STYLES = ["windows", "mac"] as const;
export type DesktopStyle = (typeof DESKTOP_STYLES)[number];
export const DEFAULT_DESKTOP: DesktopStyle = "windows";

const YEAR = 60 * 60 * 24 * 365;

export function isDesktopStyle(value: unknown): value is DesktopStyle {
  return typeof value === "string" && (DESKTOP_STYLES as readonly string[]).includes(value);
}

export function desktopStyleFrom(value: string | undefined): DesktopStyle {
  return isDesktopStyle(value) ? value : DEFAULT_DESKTOP;
}

export function writeDesktopStyle(style: DesktopStyle): void {
  document.cookie = `${DESKTOP_COOKIE}=${style}; path=/; max-age=${YEAR}; samesite=lax`;
}
