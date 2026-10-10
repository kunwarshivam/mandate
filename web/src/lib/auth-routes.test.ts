// @vitest-environment node
import { describe, expect, it } from "vitest";
import { type AuthRoute, authRoute, isPublicPath, loginHref, safeNext } from "./auth-routes";

const at = (path: string, signedIn: boolean): AuthRoute => authRoute(new URL(path, "http://127.0.0.1:4317"), signedIn);

describe("safeNext keeps `next` on this site (no open redirect)", () => {
  it.each([
    ["/agents", "/agents"],
    ["/agents/agt_01/positions", "/agents/agt_01/positions"],
    ["/approvals?filter=open", "/approvals?filter=open"],
    ["/settings/profile#passkeys", "/settings/profile#passkeys"],
    ["/", "/"],
    ["/welcome", "/welcome"],
    ["/%2F%2Fevil.example", "/%2F%2Fevil.example"],
    ["/./agents", "/agents"],
    ["/a/../agents", "/agents"],
  ])("keeps %s as %s", (next, expected) => {
    expect(safeNext(next)).toBe(expected);
  });

  it.each([
    [null],
    [undefined],
    [""],
    ["agents"],
    ["//evil.example"],
    ["//evil.example/agents"],
    ["///evil.example"],
    ["/\\evil.example"],
    ["\\\\evil.example"],
    ["/\\/evil.example"],
    ["https://evil.example"],
    ["http://127.0.0.1:4317/agents"],
    ["javascript:alert(1)"],
    ["data:text/html,hi"],
    ["/\t/evil.example"],
    ["/\n/evil.example"],
    ["/\r\n/evil.example"],
    [" /agents"],
    ["/agents evil"],
    ["/\u0000/evil.example"],
    ["/\u007f"],
    ["/login"],
    ["/login?next=/agents"],
    ["/auth/callback?code=x"],
    ["/auth/passkey"],
  ])("refuses %j and falls back to /", (next) => {
    expect(safeNext(next)).toBe("/");
  });

  it("builds the sign-in address with the path readable and the rest encoded", () => {
    expect(loginHref("/agents")).toBe("/login?next=/agents");
    expect(loginHref("/approvals?filter=open&x=1")).toBe("/login?next=/approvals%3Ffilter%3Dopen%26x%3D1");
    expect(loginHref("/")).toBe("/login");
    expect(loginHref("//evil.example")).toBe("/login");
  });

  it("round-trips: what loginHref encodes, the login page reads back as the same path", () => {
    for (const path of ["/agents", "/approvals?filter=open&x=1", "/settings/profile#passkeys"]) {
      const next = new URL(loginHref(path), "http://127.0.0.1").searchParams.get("next");
      expect(safeNext(next)).toBe(path);
    }
  });
});

describe("public paths", () => {
  it.each(["/welcome", "/login", "/api/beta", "/demo", "/auth/callback", "/auth/passkey", "/_next/static/chunks/a.js", "/favicon.ico", "/favicon.svg", "/favicon-32.png", "/apple-touch-icon.png", "/pwa-192.png", "/pwa-maskable-512.png", "/og-image.png", "/site.webmanifest", "/robots.txt"])(
    "%s is public",
    (path) => expect(isPublicPath(path)).toBe(true),
  );

  it.each(["/", "/agents", "/approvals/apr_01", "/settings/profile", "/design", "/welcome/x", "/login/x", "/auth", "/loginx", "/og-image.png.html", "/agents/x.png", "/site", "/sitex/a.png", "/demo/", "/demo/agents", "/demox", "/demo.html", "/agents/demo"])("%s is not public", (path) => {
    expect(isPublicPath(path)).toBe(false);
  });
});

describe("authRoute", () => {
  it("rewrites a signed-out visitor at / to the welcome page, keeping the address", () => {
    expect(at("/", false)).toEqual({ kind: "rewrite", to: "/welcome" });
    expect(at("/?utm=x", false)).toEqual({ kind: "rewrite", to: "/welcome" });
  });

  it("gives a signed-in user Home at /", () => {
    expect(at("/", true)).toEqual({ kind: "pass" });
  });

  it.each(["/agents", "/approvals", "/positions", "/settings/profile", "/design", "/does-not-exist"])("sends a signed-out visitor at %s to sign in, and back after", (path) => {
    expect(at(path, false)).toEqual({ kind: "redirect", to: `/login?next=${path}` });
    expect(at(path, true)).toEqual({ kind: "pass" });
  });

  it("keeps the query of the screen a signed-out visitor asked for", () => {
    expect(at("/approvals?filter=open", false)).toEqual({ kind: "redirect", to: "/login?next=/approvals%3Ffilter%3Dopen" });
  });

  it("serves the example app to a signed-out visitor at /demo alone, and no app address behind it (DEC-906)", () => {
    expect(at("/demo", false)).toEqual({ kind: "pass" });
    for (const path of ["/demo/agents", "/demo/approvals/apr_01", "/demo/../agents"]) expect(at(path, false).kind, path).toBe("redirect");
  });

  it("shows the sign-in page to a signed-out visitor", () => {
    expect(at("/login", false)).toEqual({ kind: "pass" });
    expect(at("/login?next=/agents", false)).toEqual({ kind: "pass" });
  });

  it("sends a signed-in user at /login on to next, or to /", () => {
    expect(at("/login", true)).toEqual({ kind: "redirect", to: "/" });
    expect(at("/login?next=/agents", true)).toEqual({ kind: "redirect", to: "/agents" });
    expect(at("/login?next=%2Fapprovals%3Ffilter%3Dopen", true)).toEqual({ kind: "redirect", to: "/approvals?filter=open" });
  });

  it.each(["//evil.example", "https://evil.example", "/\\evil.example", "%2F%2Fevil.example", "/login", "javascript:alert(1)"])(
    "never sends a signed-in user from /login to next=%s",
    (next) => {
      expect(at(`/login?next=${encodeURIComponent(next)}`, true)).toEqual({ kind: "redirect", to: "/" });
    },
  );

  it.each(["/welcome", "/auth/callback?code=abc", "/auth/passkey", "/og-image.png", "/site.webmanifest", "/robots.txt", "/favicon.ico"])("serves %s signed in or out", (path) => {
    expect(at(path, false)).toEqual({ kind: "pass" });
    expect(at(path, true)).toEqual({ kind: "pass" });
  });
});
