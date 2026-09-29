// @vitest-environment node
import { NextRequest } from "next/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GET } from "@/app/(site)/auth/callback/route";
import type * as AuthConfig from "./auth-config";

const supabase = vi.hoisted(() => ({
  exchangeError: null as { message: string } | null,
  passkeys: [] as { id: string }[] | null,
  listError: null as { message: string } | null,
  codes: [] as string[],
}));

vi.mock("@/lib/auth-config", async (original) => ({ ...(await original<typeof AuthConfig>()), authEnabled: true }));
vi.mock("@/lib/supabase/server", () => ({
  createClient: async () => ({
    auth: {
      exchangeCodeForSession: async (code: string) => {
        supabase.codes.push(code);
        return { data: {}, error: supabase.exchangeError };
      },
      passkey: { list: async () => ({ data: supabase.passkeys, error: supabase.listError }) },
    },
  }),
}));

const callback = (query: string) => GET(new NextRequest(`http://localhost:4317/auth/callback${query}`));

beforeEach(() => {
  supabase.exchangeError = null;
  supabase.passkeys = [{ id: "pk" }];
  supabase.listError = null;
  supabase.codes = [];
});

describe("the sign-in callback", () => {
  it("turns the code into a session and goes on to a safe next, with a relative location", async () => {
    const response = await callback("?code=abc&next=/approvals");
    expect(supabase.codes).toEqual(["abc"]);
    expect(response.status).toBe(303);
    expect(response.headers.get("location")).toBe("/approvals");
    expect(response.headers.get("cache-control")).toContain("no-store");
  });

  it("never follows next off the site", async () => {
    for (const next of ["//evil.example", "https://evil.example", "/\\evil.example", "/login"]) {
      const response = await callback(`?code=abc&next=${encodeURIComponent(next)}`);
      expect(response.headers.get("location"), next).toBe("/");
    }
  });

  it("offers the passkey step when the account has none yet", async () => {
    supabase.passkeys = [];
    const response = await callback("?code=abc&next=/agents");
    expect(response.headers.get("location")).toBe("/auth/passkey?next=%2Fagents");
  });

  it("skips the passkey step when the list can't be read", async () => {
    supabase.passkeys = null;
    supabase.listError = { message: "passkey_disabled" };
    expect((await callback("?code=abc")).headers.get("location")).toBe("/");
  });

  it("goes back to sign-in with one generic error when anything is missing or fails", async () => {
    expect((await callback("")).headers.get("location")).toBe("/login?error=1");
    expect((await callback("?error=access_denied&error_description=x")).headers.get("location")).toBe("/login?error=1");
    supabase.exchangeError = { message: "invalid grant" };
    expect((await callback("?code=bad")).headers.get("location")).toBe("/login?error=1");
  });
});
