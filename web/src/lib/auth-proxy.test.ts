// @vitest-environment node
import { NextRequest, NextResponse } from "next/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { middleware as proxy } from "@/middleware";
import type * as AuthConfig from "./auth-config";
import type * as SupabaseProxy from "./supabase/proxy";

const session = vi.hoisted(() => ({ signedIn: false, refresh: false }));

vi.mock("@/lib/auth-config", async (original) => ({ ...(await original<typeof AuthConfig>()), authEnabled: true }));
vi.mock("@/lib/supabase/proxy", async (original) => {
  const actual = await original<typeof SupabaseProxy>();
  return {
    ...actual,
    updateSession: async (request: NextRequest) => {
      const response = NextResponse.next({ request });
      if (session.refresh) {
        response.cookies.set("sb-test-auth-token", "refreshed", { path: "/", httpOnly: true });
        response.headers.set("cache-control", "private, no-cache, no-store, must-revalidate, max-age=0");
      }
      return { response, signedIn: session.signedIn };
    },
  };
});

/** `nextUrl` spells every loopback host `localhost`; Next makes a same-origin location relative after the proxy. */
const request = (path: string) => new NextRequest(`http://localhost:4317${path}`);

beforeEach(() => {
  session.signedIn = false;
  session.refresh = false;
});

describe("the proxy with sign-in on", () => {
  it("rewrites / to the welcome page for a signed-out visitor, so the address stays /", async () => {
    const response = await proxy(request("/"));
    expect(response.headers.get("x-middleware-rewrite")).toBe("http://localhost:4317/welcome");
    expect(response.headers.get("location")).toBeNull();
  });

  it("redirects a signed-out visitor from a screen to /login?next=", async () => {
    const response = await proxy(request("/agents"));
    expect(response.status).toBe(307);
    expect(response.headers.get("location")).toBe("http://localhost:4317/login?next=/agents");
  });

  it("serves Home and every screen to a signed-in user", async () => {
    session.signedIn = true;
    for (const path of ["/", "/agents", "/settings/profile"]) {
      const response = await proxy(request(path));
      expect(response.headers.get("location"), path).toBeNull();
      expect(response.headers.get("x-middleware-rewrite"), path).toBeNull();
      expect(response.headers.get("x-middleware-next"), path).toBe("1");
    }
  });

  it("sends a signed-in user from /login to a safe next only", async () => {
    session.signedIn = true;
    expect((await proxy(request("/login?next=/approvals"))).headers.get("location")).toBe("http://localhost:4317/approvals");
    expect((await proxy(request("/login?next=//evil.example"))).headers.get("location")).toBe("http://localhost:4317/");
    expect((await proxy(request("/login?next=https://evil.example"))).headers.get("location")).toBe("http://localhost:4317/");
  });

  it("carries a refreshed session and its cache headers onto a redirect and a rewrite", async () => {
    session.refresh = true;
    for (const path of ["/", "/agents"]) {
      const response = await proxy(request(path));
      expect(response.headers.getSetCookie().join("; "), path).toContain("sb-test-auth-token=refreshed");
      expect(response.headers.get("cache-control"), path).toContain("no-store");
    }
  });

  it("leaves the public pages open", async () => {
    for (const path of ["/welcome", "/login", "/auth/callback?code=x", "/auth/passkey"]) {
      const response = await proxy(request(path));
      expect(response.headers.get("location"), path).toBeNull();
      expect(response.headers.get("x-middleware-rewrite"), path).toBeNull();
    }
  });
});
