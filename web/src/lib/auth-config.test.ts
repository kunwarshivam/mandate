// @vitest-environment node
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { authEnabled, authOn, emailSignInEnabled, emailSignInOn } from "./auth-config";

const WEB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const URL = "https://project.supabase.co";
const KEY = "sb_publishable_x";

describe("authEnabled (DEC-211)", () => {
  it("is on only with both Supabase variables and the e2e flag off", () => {
    expect(authOn(URL, KEY, undefined)).toBe(true);
    expect(authOn(URL, KEY, "0")).toBe(true);
    expect(authOn(URL, KEY, "")).toBe(true);
  });

  it("is off when either variable is missing or blank", () => {
    for (const [url, key] of [
      [undefined, KEY],
      [URL, undefined],
      [undefined, undefined],
      ["", KEY],
      [URL, ""],
      ["  ", KEY],
      [URL, " \n"],
    ] as const) {
      expect(authOn(url, key, undefined), `${url} ${key}`).toBe(false);
    }
  });

  it("is off in the e2e build, whatever the variables say, so the fixtures drive every test", () => {
    expect(authOn(URL, KEY, "1")).toBe(false);
  });

  it("is off in the unit tests, which set neither variable", () => {
    expect(authEnabled).toBe(false);
  });

  it("reads the variables by their literal names, so Next inlines them into the browser bundle", () => {
    const source = readFileSync(join(WEB, "src/lib/auth-config.ts"), "utf8");
    expect(source).toContain(
      "authOn(process.env.NEXT_PUBLIC_SUPABASE_URL, process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY, process.env.OWLHEAD_E2E_SCENARIOS)",
    );
    expect(source).toContain("emailSignInOn(process.env.NEXT_PUBLIC_OWLHEAD_EMAIL_SIGNIN)");
  });
});

describe("the email sign-in flag", () => {
  it("is on only when exactly 1", () => {
    expect(emailSignInOn("1")).toBe(true);
    for (const flag of [undefined, "", "0", "true", " 1"]) expect(emailSignInOn(flag), String(flag)).toBe(false);
    expect(emailSignInEnabled).toBe(false);
  });
});

describe("the Supabase clients", () => {
  const read = (path: string) => readFileSync(join(WEB, path), "utf8");

  it("opt into passkeys in the browser client", () => {
    expect(read("src/lib/supabase/client.ts")).toMatch(/createBrowserClient\([^]*auth: \{ experimental: \{ passkey: true \} \}/);
  });

  it("decide who is signed in from verified claims, never from getSession()", () => {
    for (const path of [
      "src/lib/supabase/server.ts",
      "src/lib/supabase/proxy.ts",
      "src/middleware.ts",
      "src/app/(site)/auth/callback/route.ts",
    ]) {
      expect(read(path), path).not.toMatch(/getSession\(/);
    }
    expect(read("src/lib/supabase/proxy.ts")).toContain("await supabase.auth.getClaims()");
    expect(read("src/lib/supabase/server.ts")).toContain("await supabase.auth.getClaims()");
  });
});
