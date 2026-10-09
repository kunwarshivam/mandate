// @vitest-environment node
import { NextRequest, NextResponse } from "next/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { config, middleware } from "@/middleware";
import type * as AuthConfig from "./auth-config";
import type * as ColourPref from "./colour-pref";
import { CVD_COOKIE } from "./colour-pref";
import type * as Scenario from "./scenario";
import { SCENARIO_COOKIE } from "./scenario";
import type * as SupabaseProxy from "./supabase/proxy";

const session = vi.hoisted(() => ({ auth: true, signedIn: false, refresh: false, calls: 0 }));
/** The dev switches, as `next dev` turns them on; off as in every other build unless a test says so. */
const dev = vi.hoisted(() => ({ scenarios: false, cvd: false }));

vi.mock("@/lib/auth-config", async (original) => ({
  ...(await original<typeof AuthConfig>()),
  get authEnabled() {
    return session.auth;
  },
}));
vi.mock("@/lib/scenario", async (original) => ({
  ...(await original<typeof Scenario>()),
  get scenariosEnabled() {
    return dev.scenarios;
  },
}));
vi.mock("@/lib/colour-pref", async (original) => ({
  ...(await original<typeof ColourPref>()),
  get colourBlindEnabled() {
    return dev.cvd;
  },
}));
vi.mock("@/lib/supabase/proxy", async (original) => {
  const actual = await original<typeof SupabaseProxy>();
  return {
    ...actual,
    updateSession: async (request: NextRequest) => {
      session.calls += 1;
      const response = NextResponse.next({ request });
      if (session.refresh) {
        response.cookies.set("sb-test-auth-token", "refreshed", { path: "/", httpOnly: true });
        response.headers.set("cache-control", "private, no-cache, no-store, must-revalidate, max-age=0");
      }
      return { response, signedIn: session.signedIn };
    },
  };
});

/** `nextUrl` spells every loopback host `localhost`; Next makes a same-origin location relative after the middleware. */
const request = (path: string) => new NextRequest(`http://localhost:4317${path}`);

/** Next compiles each matcher source to a whole-path pattern; this one has no named parameters. */
const matched = (path: string) => config.matcher.some((source) => new RegExp(`^${source}$`).test(path));

beforeEach(() => {
  session.auth = true;
  session.signedIn = false;
  session.refresh = false;
  session.calls = 0;
  dev.scenarios = false;
  dev.cvd = false;
});

const cookies = (response: Response) => response.headers.getSetCookie().join("; ");

describe("the edge middleware's dev preference switch (next dev and the e2e build)", () => {
  it("pending E11-9: sets the scenario and colour cookies from ?scenario= and ?cvd=, and redirects without them", async () => {
    dev.scenarios = true;
    dev.cvd = true;
    for (const auth of [false, true]) {
      session.auth = auth;
      session.signedIn = true;
      const response = await middleware(request("/agents?scenario=stale&cvd=1&tab=open"));
      expect(response.status, String(auth)).toBe(307);
      expect(response.headers.get("location"), String(auth)).toBe("http://localhost:4317/agents?tab=open");
      expect(cookies(response), String(auth)).toContain(`${SCENARIO_COOKIE}=stale`);
      expect(cookies(response), String(auth)).toContain(`${CVD_COOKIE}=on`);
    }
    const off = await middleware(request("/?cvd=0"));
    expect(off.headers.get("location")).toBe("http://localhost:4317/");
    expect(cookies(off)).toContain(`${CVD_COOKIE}=off`);
    expect(cookies(off)).not.toContain(SCENARIO_COOKIE);
  });

  it("pending E11-9: drops an unknown scenario without a cookie, and takes each switch only while it is on", async () => {
    dev.scenarios = true;
    session.auth = false;
    const unknown = await middleware(request("/agents?scenario=bogus"));
    expect(unknown.headers.get("location")).toBe("http://localhost:4317/agents");
    expect(cookies(unknown)).not.toContain(SCENARIO_COOKIE);
    const cvdOff = await middleware(request("/agents?cvd=1"));
    expect(cvdOff.headers.get("location")).toBeNull();
    expect(cookies(cvdOff)).not.toContain(CVD_COOKIE);
    dev.scenarios = false;
    dev.cvd = true;
    const scenarioOff = await middleware(request("/agents?scenario=stale"));
    expect(scenarioOff.headers.get("location")).toBeNull();
    expect(cookies(scenarioOff)).not.toContain(SCENARIO_COOKIE);
  });

  it("pending E11-9: carries a refreshed session onto the preference redirect, before any sign-in route", async () => {
    dev.scenarios = true;
    session.refresh = true;
    session.signedIn = false;
    const response = await middleware(request("/agents?scenario=stale"));
    expect(response.headers.get("location")).toBe("http://localhost:4317/agents");
    expect(cookies(response)).toContain("sb-test-auth-token=refreshed");
    expect(cookies(response)).toContain(`${SCENARIO_COOKIE}=stale`);
    expect(response.headers.get("cache-control")).toContain("no-store");
    expect(session.calls).toBe(1);
  });
});

describe("the edge middleware with sign-in on (DEC-823: it replaces the Node proxy, which OpenNext cannot run)", () => {
  it("pending E11-9: rewrites / to the welcome page for a signed-out visitor, so the address stays /", async () => {
    const response = await middleware(request("/"));
    expect(response.headers.get("x-middleware-rewrite")).toBe("http://localhost:4317/welcome");
    expect(response.headers.get("location")).toBeNull();
    expect(session.calls).toBe(1);
  });

  it("pending E11-9: redirects a signed-out visitor from a screen to /login?next=", async () => {
    for (const path of ["/agents", "/settings/profile", "/approvals/apr_01"]) {
      const response = await middleware(request(path));
      expect(response.status, path).toBe(307);
      expect(response.headers.get("location"), path).toBe(`http://localhost:4317/login?next=${path}`);
    }
  });

  it("pending E11-9: serves Home and every screen to a signed-in user", async () => {
    session.signedIn = true;
    for (const path of ["/", "/agents", "/settings/profile"]) {
      const response = await middleware(request(path));
      expect(response.headers.get("location"), path).toBeNull();
      expect(response.headers.get("x-middleware-rewrite"), path).toBeNull();
      expect(response.headers.get("x-middleware-next"), path).toBe("1");
    }
  });

  it("pending E11-9: sends a signed-in user from /login to a safe next only", async () => {
    session.signedIn = true;
    expect((await middleware(request("/login?next=/approvals"))).headers.get("location")).toBe("http://localhost:4317/approvals");
    expect((await middleware(request("/login?next=//evil.example"))).headers.get("location")).toBe("http://localhost:4317/");
    expect((await middleware(request("/login?next=https://evil.example"))).headers.get("location")).toBe("http://localhost:4317/");
  });

  it("pending E11-9: carries a refreshed session and its cache headers onto a redirect and a rewrite", async () => {
    session.refresh = true;
    for (const path of ["/", "/agents"]) {
      const response = await middleware(request(path));
      expect(response.headers.getSetCookie().join("; "), path).toContain("sb-test-auth-token=refreshed");
      expect(response.headers.get("cache-control"), path).toContain("no-store");
    }
  });

  it("pending E11-9: leaves the public pages open", async () => {
    for (const path of ["/welcome", "/login", "/auth/callback?code=x", "/auth/passkey"]) {
      const response = await middleware(request(path));
      expect(response.headers.get("location"), path).toBeNull();
      expect(response.headers.get("x-middleware-rewrite"), path).toBeNull();
    }
  });

  it("pending E11-9: runs on every screen and the beta address, and never on the build's assets", async () => {
    const response = await middleware(request("/positions"));
    expect(response.headers.get("location")).toBe("http://localhost:4317/login?next=/positions");
    for (const path of ["/", "/agents", "/login", "/welcome", "/api/beta", "/auth/callback", "/settings/profile"]) expect(matched(path), path).toBe(true);
    for (const path of ["/_next/static/chunks/app.js", "/art/desk.png", "/video/tour.mp4", "/favicon.ico", "/favicon-32.png", "/apple-touch-icon.png", "/pwa-192.png", "/og-image.png", "/site.webmanifest", "/robots.txt"]) {
      expect(matched(path), path).toBe(false);
    }
  });
});

describe("the edge middleware with sign-in off", () => {
  it("pending E11-9: passes every request through without asking Supabase", async () => {
    session.auth = false;
    for (const path of ["/", "/agents", "/login"]) {
      const response = await middleware(request(path));
      expect(response.headers.get("x-middleware-next"), path).toBe("1");
      expect(response.headers.get("location"), path).toBeNull();
      expect(response.headers.get("x-middleware-rewrite"), path).toBeNull();
    }
    expect(session.calls).toBe(0);
  });

  it("pending E11-9: ignores ?scenario= outside development and the e2e build, and sets no scenario cookie", async () => {
    for (const auth of [false, true]) {
      session.auth = auth;
      session.signedIn = true;
      const response = await middleware(new NextRequest("http://127.0.0.1:4317/agents?scenario=stale"));
      expect(response.status, String(auth)).toBe(200);
      expect(response.headers.get("location"), String(auth)).toBeNull();
      expect(response.headers.getSetCookie().join("; "), String(auth)).not.toContain(SCENARIO_COOKIE);
    }
  });
});
