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

export function hostsFrom(site: string | undefined, app: string | undefined): Hosts | null {
  void site;
  void app;
  throw new Error("Unimplemented: E21-38");
}

export const HOSTS = null as Hosts | null;

export function hostRoute(url: URL, hosts: Hosts | null): HostRoute {
  void url;
  void hosts;
  throw new Error("Unimplemented: E21-38");
}

/** Where a sign-in link on the public site points: the app origin's sign-in page, or `/login`. */
export function signInHref(hosts: Hosts | null): string {
  void hosts;
  throw new Error("Unimplemented: E21-38");
}
