import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * DEC-2411: the build flag that lets the browser try a passkey in its cool-off is set by the local
 * cell's Playwright config alone, and read by `passkey.ts` alone. Any other file that names it, in
 * the tree or in a `web/.env*` on disk (next.config, a Dockerfile, a workflow, a release build), is
 * refused here, so no deployed build can carry it.
 */
const FLAG = "NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED";
const ROOT = join(__dirname, "..", "..", "..");
const ALLOWED = new Set(["web/playwright.cell.config.ts", "web/src/api/passkey.ts", "web/src/test/cool-off-waiver-static.test.ts"]);

function tracked(): string[] {
  return execFileSync("git", ["ls-files", "-z"], { cwd: ROOT, encoding: "utf8" }).split("\0").filter(Boolean);
}

function envFiles(): string[] {
  const web = join(ROOT, "web");
  return readdirSync(web)
    .filter((name) => name.startsWith(".env"))
    .map((name) => `web/${name}`);
}

/** A file's text, or nothing for a path that is gone, not a file, or too large to be a setting. */
function fileText(path: string): string | null {
  const full = join(ROOT, path);
  if (!existsSync(full) || !statSync(full).isFile() || statSync(full).size > 4_000_000) return null;
  return readFileSync(full, "utf8");
}

/** The files outside the allow list that name the flag; Markdown is prose about it, never a setting. */
function naming(paths: readonly string[], read: (path: string) => string | null = fileText): string[] {
  return paths.filter((path) => !ALLOWED.has(path) && !path.endsWith(".md") && (read(path) ?? "").includes(FLAG));
}

describe("the cool-off waiver flag (DEC-2411)", () => {
  it("is named by no file but the cell's Playwright config and its one reader", () => {
    expect(naming([...new Set([...tracked(), ...envFiles()])])).toEqual([]);
  });

  it("is set by the cell config only when the driver says the cell waives the cool-off", () => {
    const config = readFileSync(join(ROOT, "web/playwright.cell.config.ts"), "utf8");
    expect(config).toContain(`${FLAG}: "1"`);
    expect(config).toMatch(/OWLHEAD_CELL_COOL_OFF_WAIVED === "1"/);
  });

  it("refuses any other file that sets it, and lets the allowed ones and prose name it", () => {
    const setting = `env: { ${FLAG}: "1" }`;
    const seeded = ["web/next.config.ts", "web/.env.production", "web/Dockerfile", ".github/workflows/release.yml", "web/playwright.config.ts"];
    expect(naming(seeded, () => setting)).toEqual(seeded);
    expect(naming([...ALLOWED, "docs/project/decisions/DEC-2411.md"], () => setting)).toEqual([]);
    expect(naming(["web/next.config.ts"], () => "env: {}")).toEqual([]);
  });
});
