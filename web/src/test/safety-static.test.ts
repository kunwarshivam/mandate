// @vitest-environment node
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

const WEB = join(__dirname, "..", "..");
const SRC = join(WEB, "src");

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

const SOURCES = files(SRC).filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f));

async function lint(code: string) {
  const eslint = new ESLint({ cwd: WEB });
  const [result] = await eslint.lintText(code, { filePath: join(SRC, "components", "lint-probe.tsx") });
  return result.messages.filter((m) => m.ruleId === "no-restricted-syntax" || m.ruleId === "no-restricted-imports").map((m) => m.message);
}

describe("lint bans", () => {
  it.each([
    [`<Button variant="destructive">Go</Button>`, /destructive variant is banned/],
    [`<Button variant="secondary-destructive">Go</Button>`, /secondary-destructive variant is banned/],
    [`<Button variant={"destructive"}>Go</Button>`, /destructive variants are banned/],
    [`<KillSwitchButton title="Kill" disabled onClick={() => {}} />`, /never disabled or loading/],
    [`<StopControl loading />`, /never disabled or loading/],
  ])("rejects %s", async (jsx, message) => {
    const messages = await lint(`import { Button } from "@cloudflare/kumo/components/button";\nexport const X = () => ${jsx};\n`);
    expect(messages.some((m) => message.test(m))).toBe(true);
  }, 30_000);

  it("rejects the root Kumo barrel", async () => {
    const messages = await lint(`import { Button } from "@cloudflare/kumo";\nexport const X = () => <Button>Go</Button>;\n`);
    expect(messages.some((m) => /per component/.test(m))).toBe(true);
  }, 30_000);

  it("accepts a primary button", async () => {
    const messages = await lint(`import { Button } from "@cloudflare/kumo/components/button";\nexport const X = () => <Button variant="primary">Go</Button>;\n`);
    expect(messages).toEqual([]);
  }, 30_000);
});

describe("banned parts", () => {
  const banned: Array<[string, RegExp]> = [
    ["Cloudflare branding", /CloudflareLogo|PoweredByCloudflare/],
    ["clipboard copy", /navigator\.clipboard|Clipboard|CopyButton|ClipboardText/],
    ["meters for goals or limits", /components\/meter|<Meter\b/],
    ["browser storage", /localStorage|sessionStorage|indexedDB/],
    ["typed confirmation", /type (the )?(name|word) to confirm/i],
    ["recommendation badges", />\s*(Recommended|New)\s*</],
    ["select all", /select all/i],
    ["a recents list", /["'>]\s*Recents?\s*["'<]|recentItems|useRecent/],
  ];

  it.each(banned)("uses no %s", (_what, pattern) => {
    const hits = SOURCES.filter((f) => pattern.test(readFileSync(f, "utf8"))).map((f) => relative(WEB, f));
    expect(hits).toEqual([]);
  });

  it("never draws Kumo's destructive or loading states on a Stop surface", () => {
    for (const f of SOURCES.filter((x) => /components\/(stop|shell)\//.test(x))) {
      const text = readFileSync(f, "utf8");
      expect(text, relative(WEB, f)).not.toMatch(/\bloading=\{?/);
      expect(text, relative(WEB, f)).not.toMatch(/variant="(secondary-)?destructive"/);
    }
  });
});

describe("flat fills over Kumo", () => {
  const css = readFileSync(join(SRC, "app", "placard-kumo.css"), "utf8");
  const kumo = files(join(WEB, "node_modules", "@cloudflare", "kumo", "dist"))
    .filter((f) => /\.(js|css)$/.test(f))
    .map((f) => readFileSync(f, "utf8"))
    .join("\n");

  it.each([
    ["the emphasis overlay on buttons", '[data-kumo-component="Button"] > span[aria-hidden="true"].absolute'],
    ["linear fills on any element", '[class*="linear-to-"]'],
    ["the sticky table fade", '[class*="before:from-transparent"]::before'],
    ["scroll masks", '[class*="mask-image"]'],
    ["the skeleton shimmer", ".skeleton-line::after"],
    ["the chart shimmer", ".kumo-chart-shimmer"],
  ])("overrides %s", (_what, selector) => {
    expect(css).toContain(selector);
  });

  it("targets class names Kumo still ships, so an upgrade cannot silently bring the effects back", () => {
    for (const hook of ["linear-to-", "before:from-transparent", "mask-image", "skeleton-line", "kumo-chart-shimmer", 'data-kumo-component']) {
      expect(kumo, hook).toContain(hook);
    }
  });

  it("paints the overlay as one solid brand colour, with no image and no shadow", () => {
    const rule = css.slice(css.indexOf('[data-kumo-component="Button"] > span[aria-hidden="true"].absolute'));
    const body = rule.slice(rule.indexOf("{"), rule.indexOf("}"));
    expect(body).toMatch(/background-image:\s*none/);
    expect(body).toMatch(/background-color:\s*var\(--color-kumo-brand\)/);
    expect(body).toMatch(/box-shadow:\s*none/);
  });

  it("squares every corner Kumo rounds", () => {
    for (const selector of [".rounded", ".rounded-full", ".rounded-\\[5px\\]", ".rounded-\\[10px\\]"]) expect(css).toContain(selector);
  });

  it("keeps crimson for the kill switch alone", () => {
    const crimson = SOURCES.filter((f) => /crimson/.test(readFileSync(f, "utf8"))).map((f) => relative(WEB, f));
    for (const f of crimson) expect(f).toMatch(/kill-switch-button|stop-sheet|lib\/(tokens|palette|contrast-pairs)\.ts|design\//);
  });
});
