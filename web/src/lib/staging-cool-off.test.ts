// @vitest-environment node
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { usablePasskeys } from "@/api/passkey";
import { STAGING_WAIVER_CEILING, stagingWaived, stagingWaiverUntil } from "./cool-off-build";

/**
 * The staging demo's cool-off waiver in the browser (DEC-2150): a build may carry an instant, no
 * later than the server's compiled ceiling, until which the browser tries a passkey still cooling off
 * and lets the staging server judge it. Past the instant it is inert; a malformed or later one stops
 * the build.
 */
const ROOT = join(__dirname, "..", "..", "..");
const text = (path: string) => readFileSync(join(ROOT, path), "utf8");
const FLAG = "NEXT_PUBLIC_OWLHEAD_STAGING_COOL_OFF_UNTIL";

describe("the staging waiver's instant", () => {
  it("is unset, or a whole UTC second no later than the ceiling", () => {
    expect(stagingWaiverUntil(undefined)).toBeNull();
    expect(stagingWaiverUntil("")).toBeNull();
    expect(stagingWaiverUntil("2026-11-01T12:00:00Z")).toBe(Date.parse("2026-11-01T12:00:00Z"));
    expect(stagingWaiverUntil(STAGING_WAIVER_CEILING)).toBe(Date.parse(STAGING_WAIVER_CEILING));
  });

  it("stops the build for one past the ceiling or in any other form", () => {
    expect(() => stagingWaiverUntil("2026-11-05T00:00:01Z")).toThrow(/later than the ceiling/);
    for (const value of ["2026-11-01T12:00:00.000Z", "2026-11-01 12:00:00Z", "2026-11-01T12:00:00+00:00", "2026-02-30T00:00:00Z", "1", "true"]) expect(() => stagingWaiverUntil(value), value).toThrow(/must be/);
  });

  it("holds strictly before its instant, never at or after it, and never unset", () => {
    const until = Date.parse("2026-11-01T12:00:00Z");
    expect(stagingWaived(until, until - 1)).toBe(true);
    expect(stagingWaived(until, until)).toBe(false);
    expect(stagingWaived(until, until + 1)).toBe(false);
    expect(stagingWaived(null, 0)).toBe(false);
  });

  it("is the server's ceiling exactly, so the browser never waives past it", () => {
    expect(text("crates/mandate-api-server/src/config.rs")).toContain(`pub const STAGING_WAIVER_CEILING: &str = "${STAGING_WAIVER_CEILING}";`);
  });
});

describe("where the instant is set and read", () => {
  it("is checked by next.config.ts before the config is made, and read by passkey.ts's pre-check", () => {
    const config = text("web/next.config.ts");
    expect(config).toContain(`stagingWaiverUntil(process.env.${FLAG});`);
    expect(config.indexOf(`stagingWaiverUntil(process.env.${FLAG})`)).toBeLessThan(config.indexOf("const nextConfig"));
    expect(text("web/src/api/passkey.ts")).toContain("waived ??= COOL_OFF_WAIVED || stagingWaived(STAGING_UNTIL, at);");
  });

  it("is set by web-deploy.yml alone, to an instant the build accepts", () => {
    const deploy = text(".github/workflows/web-deploy.yml");
    const value = new RegExp(`${FLAG}: "([^"]+)"`).exec(deploy)?.[1];
    expect(value).toBeDefined();
    expect(() => stagingWaiverUntil(value)).not.toThrow();
    for (const path of [".github/workflows/web.yml", "web/playwright.config.ts", "web/playwright.cell.config.ts"]) expect(text(path), path).not.toContain(FLAG);
  });

  it("lets a cooling passkey be tried only when waived, leaving the verdict to the server", () => {
    const cooling = [{ passkey_id: "pk_1", alg: "es256" as const, cool_off_ends: "2026-10-30T00:00:00.000000000Z" }];
    const device = async () => null;
    expect(usablePasskeys(cooling, device, "2026-10-29T12:00:00.000000000Z", false)).toEqual({ ok: false, reason: "passkey_cooling_off" });
    expect(usablePasskeys(cooling, device, "2026-10-29T12:00:00.000000000Z", true)).toMatchObject({ ok: true });
    expect(usablePasskeys(cooling, device, "2026-10-29T12:00:00.000000000Z")).toEqual({ ok: false, reason: "passkey_cooling_off" });
  });
});
