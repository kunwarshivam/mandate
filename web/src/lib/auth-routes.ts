/**
 * Where a request goes while sign-in is on (DEC-211): the proxy's whole decision, as a pure function
 * of the address and whether a verified session exists, so every case is a unit test.
 */
export const LOGIN_PATH = "/login";
export const WELCOME_PATH = "/welcome";
export const CALLBACK_PATH = "/auth/callback";
export const PASSKEY_PATH = "/auth/passkey";
/**
 * The app over the example workspace, shown in the landing page's browser tab (DEC-906). Only this
 * exact path is public: it renders fixtures alone, and moves between screens in memory, so no other
 * app address ever opens signed out.
 */
export const DEMO_PATH = "/demo";
/** The private beta's request form posts here signed out (`@/lib/beta`). */
export const BETA_REQUEST_PATH = "/api/beta";

/** Files in `public/` a signed-out browser fetches: icons, the manifest, the share image and robots. */
const PUBLIC_FILES = new Set([
  "/favicon.ico",
  "/favicon.svg",
  "/favicon-16.png",
  "/favicon-32.png",
  "/favicon-48.png",
  "/apple-touch-icon.png",
  "/pwa-192.png",
  "/pwa-512.png",
  "/pwa-maskable-512.png",
  "/og-image.png",
  "/site.webmanifest",
  "/robots.txt",
  "/push-sw.js",
]);

export function isPublicPath(pathname: string): boolean {
  return (
    pathname === WELCOME_PATH ||
    pathname === LOGIN_PATH ||
    pathname === BETA_REQUEST_PATH ||
    pathname === DEMO_PATH ||
    pathname.startsWith("/auth/") ||
    pathname.startsWith("/_next/") ||
    PUBLIC_FILES.has(pathname)
  );
}

const PROBE_ORIGIN = "http://owlhead.invalid";

/**
 * `next` as a path on this site, or `/`. Only a path that starts with one slash and resolves to this
 * origin survives: `//evil`, `/\evil`, `https://…`, `javascript:` and any whitespace or control
 * character (which browsers strip, turning `/\t/evil` into `//evil`) are refused. A path back into
 * sign-in itself becomes `/`, so a redirect cannot loop.
 */
export function safeNext(next: string | null | undefined): string {
  if (!next || !next.startsWith("/") || next.startsWith("//")) return "/";
  if (/[\\\s\u0000-\u001f\u007f]/.test(next)) return "/";
  let url: URL;
  try {
    url = new URL(next, PROBE_ORIGIN);
  } catch {
    return "/";
  }
  if (url.origin !== PROBE_ORIGIN) return "/";
  if (url.pathname === LOGIN_PATH || url.pathname.startsWith("/auth/")) return "/";
  return `${url.pathname}${url.search}${url.hash}`;
}

/** `/login?next=…`, with the path's slashes left readable. */
export function loginHref(next: string): string {
  const safe = safeNext(next);
  return safe === "/" ? LOGIN_PATH : `${LOGIN_PATH}?next=${encodeURIComponent(safe).replaceAll("%2F", "/")}`;
}

export type AuthRoute = { kind: "pass" } | { kind: "rewrite"; to: string } | { kind: "redirect"; to: string };

/**
 * - `/login`: signed in, on to `next` (or `/`); signed out, the sign-in page.
 * - Other public paths: always served.
 * - `/`: signed in, Home; signed out, the welcome page under the same address.
 * - Any other path: signed in, served; signed out, to `/login?next=<path>`.
 */
export function authRoute(url: URL, signedIn: boolean): AuthRoute {
  const { pathname } = url;
  if (pathname === LOGIN_PATH) return signedIn ? { kind: "redirect", to: safeNext(url.searchParams.get("next")) } : { kind: "pass" };
  if (isPublicPath(pathname) || signedIn) return { kind: "pass" };
  if (pathname === "/") return { kind: "rewrite", to: WELCOME_PATH };
  return { kind: "redirect", to: loginHref(`${pathname}${url.search}`) };
}
