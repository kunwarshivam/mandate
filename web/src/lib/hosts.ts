import { LOGIN_PATH, WELCOME_PATH, isSitePath } from "@/lib/auth-routes";

/**
 * The two origins the app is served from (DEC-2405): the public site and the app. Sign-in, its
 * callbacks and every app screen live on the app origin alone, so the session cookie, which stays
 * host-only, is set and read in one place. Both come from the build (`NEXT_PUBLIC_OWLHEAD_SITE_ORIGIN`,
 * `NEXT_PUBLIC_OWLHEAD_APP_ORIGIN`); with either unset, or the two not distinct origins, nothing
 * is routed by host, as in development and the tests.
 */
export type Hosts = { site: URL; app: URL };

export type HostRoute =
  | { kind: "app" }
  | { kind: "pass" }
  | { kind: "rewrite"; to: string }
  | { kind: "redirect"; to: string };

/** An `http(s)` origin and nothing else (no path, query or credentials), or null. */
function origin(value: string | undefined): URL | null {
  if (!value) return null;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  const bare = url.pathname === "/" && !url.search && !url.hash && !url.username && !url.password;
  return (url.protocol === "https:" || url.protocol === "http:") && bare && value.replace(/\/$/, "") === url.origin ? url : null;
}

export function hostsFrom(site: string | undefined, app: string | undefined): Hosts | null {
  const s = origin(site);
  const a = origin(app);
  return s && a && s.origin !== a.origin ? { site: s, app: a } : null;
}

export const HOSTS = hostsFrom(process.env.NEXT_PUBLIC_OWLHEAD_SITE_ORIGIN, process.env.NEXT_PUBLIC_OWLHEAD_APP_ORIGIN);

/**
 * The same path and query on `base`'s origin, and never another origin: a path that begins with more
 * than one slash (`//evil.com`, or `/\evil.com`, which a URL parser reads as `//evil.com`) would
 * otherwise be read as a host, so leading slashes collapse to one and the result is checked, falling
 * back to the origin's root.
 */
function moved(url: URL, base: URL): string {
  const path = url.pathname.replace(/^\/+/, "/");
  const target = new URL(`${base.origin}${path}${url.search}`);
  return target.origin === base.origin ? target.href : `${base.origin}/`;
}

/**
 * - `www.` and the site's host: to the site, path and query kept.
 * - The site: its public pages served with no session (`/` as the welcome page); anything else, sign-in
 *   and its callbacks included, to the same path and query on the app.
 * - The app, any other host, or no origins configured: the sign-in rules decide (`authRoute`).
 */
export function hostRoute(url: URL, hosts: Hosts | null): HostRoute {
  if (!hosts) return { kind: "app" };
  const host = url.host.toLowerCase();
  if (host === `www.${hosts.site.host}`) return { kind: "redirect", to: moved(url, hosts.site) };
  if (host !== hosts.site.host) return { kind: "app" };
  if (!isSitePath(url.pathname)) return { kind: "redirect", to: moved(url, hosts.app) };
  return url.pathname === "/" ? { kind: "rewrite", to: WELCOME_PATH } : { kind: "pass" };
}

/** Where a sign-in link on the public site points: the app origin's sign-in page, or `/login`. */
export function signInHref(hosts: Hosts | null): string {
  return hosts ? new URL(LOGIN_PATH, hosts.app.origin).href : LOGIN_PATH;
}

/** The public site's sign-in links, fixed when the app is built. */
export const SIGN_IN_HREF = signInHref(HOSTS);
