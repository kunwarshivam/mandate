import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import DirectionsPage from "@/app/directions/page.dev";
import { DEV_PAGE_EXTENSION, devFrameHeaders, pageExtensions } from "./dev-routes";

const WEB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const web = (path: string) => join(WEB, path);

afterEach(() => vi.unstubAllEnvs());

describe("the /directions prototype stays out of production", () => {
  it("is compiled only when dev page extensions are on", () => {
    expect(pageExtensions("production")).not.toContain(DEV_PAGE_EXTENSION);
    expect(pageExtensions("development")).toContain(DEV_PAGE_EXTENSION);
    expect(readFileSync(web("next.config.ts"), "utf8")).toContain("pageExtensions: pageExtensions(process.env.NODE_ENV)");
  });

  it("has no route file a production build would pick up", () => {
    const files = readdirSync(web("src/app/directions"));
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
    await expect(DirectionsPage({ searchParams: Promise.resolve({}) })).rejects.toThrow("NEXT_NOT_FOUND");
    vi.stubEnv("NODE_ENV", "test");
    await expect(DirectionsPage({ searchParams: Promise.resolve({}) })).rejects.toThrow("NEXT_NOT_FOUND");
    vi.stubEnv("NODE_ENV", "development");
    await expect(DirectionsPage({ searchParams: Promise.resolve({ v: "2" }) })).resolves.toBeTruthy();
  });

  it("allows same-origin framing for it in dev only", () => {
    expect(devFrameHeaders("production")).toEqual([]);
    expect(devFrameHeaders("development")).toEqual([{ source: "/directions", headers: [{ key: "X-Frame-Options", value: "SAMEORIGIN" }] }]);
  });

  it("is not linked from the navigation", () => {
    expect(readFileSync(web("src/components/shell/nav.tsx"), "utf8")).not.toContain("/directions");
  });
});
