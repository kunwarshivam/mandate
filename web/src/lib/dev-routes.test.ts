import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import PalettePage from "@/app/(app)/palette/page.dev";
import { DEV_PAGE_EXTENSION, pageExtensions } from "./dev-routes";

const WEB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const web = (path: string) => join(WEB, path);

afterEach(() => vi.unstubAllEnvs());

describe("the /palette reference stays out of production", () => {
  it("is compiled only when dev page extensions are on", () => {
    expect(pageExtensions("production")).not.toContain(DEV_PAGE_EXTENSION);
    expect(pageExtensions("development")).toContain(DEV_PAGE_EXTENSION);
    expect(readFileSync(web("next.config.ts"), "utf8")).toContain("pageExtensions: pageExtensions(process.env.NODE_ENV)");
  });

  it("has no route file a production build would pick up", () => {
    const files = readdirSync(web("src/app/(app)/palette"));
    expect(files).toContain("page.dev.tsx");
    const production = pageExtensions("production");
    const routeFile = /^(page|layout|route|template|default|loading|error|not-found)\.(.+)$/;
    for (const f of files) {
      const ext = routeFile.exec(f)?.[2];
      if (ext) expect(production, `${f} would be a production route`).not.toContain(ext);
    }
  });

  it("answers 404 outside development even if compiled", async () => {
    vi.stubEnv("NODE_ENV", "production");
    await expect(PalettePage()).rejects.toThrow("NEXT_NOT_FOUND");
    vi.stubEnv("NODE_ENV", "test");
    await expect(PalettePage()).rejects.toThrow("NEXT_NOT_FOUND");
    vi.stubEnv("NODE_ENV", "development");
    await expect(PalettePage()).resolves.toBeTruthy();
  });

  it("is not linked from the navigation", () => {
    expect(readFileSync(web("src/components/shell/nav.tsx"), "utf8")).not.toContain("/palette");
  });
});

describe("the colour-blind friendly preference is dev only, and there is one palette", () => {
  it("reads the cookie only in development and sets <html data-cvd> from it", () => {
    expect(readFileSync(web("src/lib/colour-pref.ts"), "utf8")).toContain('export const colourBlindEnabled = process.env.NODE_ENV === "development";');
    const workspace = readFileSync(web("src/lib/get-workspace.ts"), "utf8");
    expect(workspace).toMatch(/getColourBlind[^]*if \(!colourBlindEnabled\) return false;/);
    const layout = readFileSync(web("src/app/layout.tsx"), "utf8");
    expect(layout).toContain('data-cvd={colourBlind ? "on" : undefined}');
    expect(readFileSync(web("src/components/shell/app-frame.tsx"), "utf8")).toMatch(/\{scenarioSwitcherShown \? <ScenarioSwitcher /);
  });

  it("takes ?cvd only in development, and no ?palette at all", () => {
    const proxy = readFileSync(web("src/middleware.ts"), "utf8");
    expect(proxy).toContain("const cvd = colourBlindEnabled ? params.get(CVD_PARAM) : null;");
    expect(proxy).not.toMatch(/palette/i);
  });

  it("has no palette switch in the layout", () => {
    for (const layout of ["src/app/layout.tsx", "src/components/shell/app-frame.tsx", "src/app/(site)/layout.tsx"]) {
      expect(readFileSync(web(layout), "utf8"), layout).not.toMatch(/PaletteStyle|data-palette|DevPanel/);
    }
  });
});
