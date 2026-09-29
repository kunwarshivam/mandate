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
  // DEC-213: the signed-out landing's blinking "New" is a period news tag beside the beta notice, not a badge on a strategy or a trade.
  const SITE = /src\/components\/site\//;
  const banned: Array<[string, RegExp, RegExp?]> = [
    ["Cloudflare branding", /CloudflareLogo|PoweredByCloudflare/],
    ["clipboard copy", /navigator\.clipboard|Clipboard|CopyButton|ClipboardText/],
    ["meters for goals or limits", /components\/meter|<Meter\b/],
    ["browser storage", /localStorage|sessionStorage|indexedDB/],
    ["typed confirmation", /type (the )?(name|word) to confirm/i],
    ["recommendation badges", />\s*(Recommended|New)\s*</, SITE],
    ["select all", /select all/i],
    ["a recents list", /["'>]\s*Recents?\s*["'<]|recentItems|useRecent/],
  ];

  it.each(banned)("uses no %s", (_what, pattern, exempt) => {
    const hits = SOURCES.filter((f) => !exempt?.test(relative(WEB, f)) && pattern.test(readFileSync(f, "utf8"))).map((f) => relative(WEB, f));
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
  const css = readFileSync(join(SRC, "app", "kumo-theme.css"), "utf8");
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

  it("puts every arbitrary radius Kumo ships on the radius scale", () => {
    const arbitrary = new Set(Array.from(kumo.matchAll(/\brounded(?:-[trbl]{1,2})?-\[(\d+)px\]/g), (m) => m[0]));
    expect(arbitrary.size).toBeGreaterThan(0);
    for (const cls of arbitrary) {
      const selector = `.${cls.replace("[", "\\[").replace("]", "\\]")}`;
      const rule = css.slice(css.indexOf(`${selector} {`));
      expect(css, cls).toContain(`${selector} {`);
      expect(rule.slice(0, rule.indexOf("}")), cls).toMatch(/radius:\s*var\(--radius-(sm|lg)\)/);
    }
  });

  it("keeps crimson for the kill switch alone", () => {
    const crimson = SOURCES.filter((f) => /crimson/.test(readFileSync(f, "utf8"))).map((f) => relative(WEB, f));
    for (const f of crimson) expect(f).toMatch(/kill-switch-button|stop-sheet|lib\/(tokens|palette|contrast-pairs)\.ts|design\//);
  });
});
