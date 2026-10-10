// @vitest-environment node
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { NextRequest } from "next/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ScenarioSwitcher as SwitcherOff } from "@/components/dev/scenario-switcher.off";
import { middleware as proxy } from "@/middleware";
import nextConfig from "../../next.config";
import { AUTH_E2E_FLAG, E2E_FLAG, distDir, isE2eBuild } from "../../scripts/e2e-build.mjs";
import { SWITCHER_MARKER } from "../../scripts/no-scenarios.mjs";
import { SCENARIO_SWITCHER_MODULE, SCENARIO_SWITCHER_OFF, devOnlyAliases } from "./dev-routes";
import { SCENARIO_COOKIE, scenarioSwitcherShown, scenariosEnabled, scenariosOn } from "./scenario";

const WEB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const read = (path: string) => readFileSync(join(WEB, path), "utf8");

afterEach(() => vi.unstubAllEnvs());

describe("the fixture scenario switch is on in next dev and the e2e build only", () => {
  it("is on in development, or when the e2e flag is exactly 1", () => {
    expect(scenariosOn("development", undefined)).toBe(true);
    expect(scenariosOn("production", "1")).toBe(true);
    for (const flag of [undefined, "", "0", "true", "yes", " 1"]) expect(scenariosOn("production", flag), String(flag)).toBe(false);
    expect(scenariosOn("test", undefined)).toBe(false);
  });

  it("is off, with no switcher, outside development without the flag", () => {
    expect(process.env[E2E_FLAG]).toBeUndefined();
    expect(scenariosEnabled).toBe(false);
    expect(scenarioSwitcherShown).toBe(false);
  });

  it("ignores ?scenario= and sets no scenario cookie when it is off", async () => {
    const response = await proxy(new NextRequest("http://127.0.0.1:4317/agents?scenario=stale"));
    expect(response.status).toBe(200);
    expect(response.headers.get("location")).toBeNull();
    expect(response.headers.getSetCookie().join("; ")).not.toContain(SCENARIO_COOKIE);
  });

  it("reads the flag when the app is built: next.config.ts inlines it and gives the e2e build its own directory", () => {
    expect(nextConfig.env).toEqual({ [E2E_FLAG]: "0" });
    expect(nextConfig.distDir).toBe(".next");
    expect(read("next.config.ts")).toContain('env: { [E2E_FLAG]: isE2eBuild() ? "1" : "0" },');
    expect(read("src/lib/scenario.ts")).toContain("scenariosOn(process.env.NODE_ENV, process.env.OWLHEAD_E2E_SCENARIOS)");
    vi.stubEnv(E2E_FLAG, "1");
    expect(isE2eBuild()).toBe(true);
    expect(distDir()).toBe(".next-e2e");
    vi.stubEnv(E2E_FLAG, "0");
    expect(distDir()).toBe(".next");
    vi.stubEnv(AUTH_E2E_FLAG, "1");
    expect(distDir()).toBe(".next-auth-e2e");
    vi.stubEnv(E2E_FLAG, "1");
    expect(distDir()).toBe(".next-e2e");
  });

  it("sets the flag only for the Playwright suite's own build", () => {
    const playwright = read("playwright.config.ts");
    expect(playwright).toContain('command: "npm run build && npm run start",');
    expect(playwright).toContain('env: { [E2E_FLAG]: "1" },');
    expect(read("package.json")).toMatch(/"build": "next build",/);
    expect(read("../.github/workflows/web.yml")).not.toContain(E2E_FLAG);
  });
});

describe("a production build does not contain the scenario switcher", () => {
  it("aliases the switcher to a stub that renders nothing", () => {
    expect(devOnlyAliases("production")).toEqual({ [SCENARIO_SWITCHER_MODULE]: SCENARIO_SWITCHER_OFF });
    expect(devOnlyAliases("development")).toEqual({});
    expect(read("next.config.ts")).toContain("turbopack: { resolveAlias: devOnlyAliases(process.env.NODE_ENV) },");
    expect(read("src/components/shell/app-frame.tsx")).toContain(`from "${SCENARIO_SWITCHER_MODULE}";`);
    expect(existsSync(join(WEB, SCENARIO_SWITCHER_OFF))).toBe(true);
    expect(SwitcherOff()).toBeNull();
    expect(read(SCENARIO_SWITCHER_OFF)).not.toContain("use client");
  });

  it("marks the switcher so the build check can find it, and runs that check after every build", () => {
    expect(read("src/components/dev/scenario-switcher.tsx")).toContain(`data-slot="${SWITCHER_MARKER}"`);
    expect(read("package.json")).toContain('"postbuild": "node scripts/no-apca.mjs && node scripts/no-scenarios.mjs",');
  });
});
