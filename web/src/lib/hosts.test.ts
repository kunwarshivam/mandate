import { describe, expect, it } from "vitest";
import { authRoute } from "./auth-routes";
import { type Hosts, hostRoute, hostsFrom, signInHref } from "./hosts";

const prod = () => hostsFrom("https://owlhead.ai", "https://app.owlhead.ai") as Hosts;

/** The whole decision for one request, as the middleware takes it: by host first, then sign-in on the app. */
function decide(href: string, signedIn: boolean, hosts: Hosts | null = prod()) {
  const url = new URL(href);
  const byHost = hostRoute(url, hosts);
  return byHost.kind === "app" ? authRoute(url, signedIn) : byHost;
}

describe("the two origins (DEC-2405)", () => {
  it("pending E21-38: reads two distinct origins, and nothing else", () => {
    expect(prod()).not.toBeNull();
    expect(prod().site.origin).toBe("https://owlhead.ai");
    expect(prod().app.origin).toBe("https://app.owlhead.ai");
    for (const [site, app] of [
      [undefined, "https://app.owlhead.ai"],
      ["https://owlhead.ai", undefined],
      ["", ""],
      ["https://owlhead.ai", "https://owlhead.ai"],
      ["owlhead.ai", "app.owlhead.ai"],
      ["https://owlhead.ai/x", "https://app.owlhead.ai"],
      ["javascript:alert(1)", "https://app.owlhead.ai"],
    ] as const) {
      expect(hostsFrom(site, app), `${site} ${app}`).toBeNull();
    }
    expect(hostsFrom("http://localhost:4318", "http://127.0.0.1:4318")?.app.origin).toBe("http://127.0.0.1:4318");
  });

  it("pending E21-38: sends www to the site, keeping the path and query", () => {
    for (const signedIn of [false, true]) {
      expect(decide("https://www.owlhead.ai/", signedIn)).toEqual({ kind: "redirect", to: "https://owlhead.ai/" });
      expect(decide("https://www.owlhead.ai/login?next=/agents", signedIn)).toEqual({ kind: "redirect", to: "https://owlhead.ai/login?next=/agents" });
      expect(decide("https://WWW.Owlhead.AI/welcome", signedIn)).toEqual({ kind: "redirect", to: "https://owlhead.ai/welcome" });
    }
  });

  it("pending E21-38: serves the public site on the site, signed in or not, with no session", () => {
    for (const signedIn of [false, true]) {
      expect(decide("https://owlhead.ai/", signedIn)).toEqual({ kind: "rewrite", to: "/welcome" });
      for (const path of ["/welcome", "/api/beta", "/demo", "/favicon.ico", "/robots.txt", "/_next/static/x.js"]) {
        expect(decide(`https://owlhead.ai${path}`, signedIn), path).toEqual({ kind: "pass" });
      }
    }
  });

  it("pending E21-38: sends sign-in, its callbacks and every app screen from the site to the app, query kept", () => {
    for (const signedIn of [false, true]) {
      for (const path of ["/login", "/login?next=/agents%3Fx%3D1", "/auth/callback?code=abc&next=/approvals", "/auth/passkey", "/agents", "/approvals/apr_01?tab=why", "/settings/profile", "/v1x"]) {
        expect(decide(`https://owlhead.ai${path}`, signedIn), path).toEqual({ kind: "redirect", to: `https://app.owlhead.ai${path}` });
      }
    }
  });

  it("pending E21-38: leaves the app to the sign-in rules, signed in or not", () => {
    expect(decide("https://app.owlhead.ai/agents", false)).toEqual({ kind: "redirect", to: "/login?next=/agents" });
    expect(decide("https://app.owlhead.ai/agents", true)).toEqual({ kind: "pass" });
    expect(decide("https://app.owlhead.ai/login?next=/approvals", true)).toEqual({ kind: "redirect", to: "/approvals" });
    expect(decide("https://app.owlhead.ai/login", false)).toEqual({ kind: "pass" });
    expect(decide("https://app.owlhead.ai/", false)).toEqual({ kind: "rewrite", to: "/welcome" });
    expect(decide("https://app.owlhead.ai/", true)).toEqual({ kind: "pass" });
  });

  it("pending E21-38: routes nothing by host when the origins are unset, or on any other host", () => {
    for (const signedIn of [false, true]) {
      for (const href of ["https://owlhead.ai/agents", "https://www.owlhead.ai/", "http://127.0.0.1:4317/login", "https://owlhead-web.example.workers.dev/agents"]) {
        expect(decide(href, signedIn, null), href).toEqual(authRoute(new URL(href), signedIn));
      }
      expect(decide("https://owlhead-web.example.workers.dev/agents", signedIn)).toEqual(authRoute(new URL("https://owlhead-web.example.workers.dev/agents"), signedIn));
    }
  });

  it("pending E21-38: points the site's sign-in links at the app, or at /login with no origins", () => {
    expect(signInHref(prod())).toBe("https://app.owlhead.ai/login");
    expect(signInHref(null)).toBe("/login");
  });
});
