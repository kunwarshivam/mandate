// @vitest-environment node
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import nextConfig from "../../next.config";
import { WORKER_BUDGET, hostingProblems, sizeProblems, workerSize } from "../../scripts/workers.mjs";

const WEB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const MIB = 1024 * 1024;

type Images = { unoptimized?: boolean };

/** This tree as `hostingProblems` reads it, with files replaced (a string) or removed (`null`), and its Next config's images. */
function tree(changes: Record<string, string | null> = {}, config: { images?: Images } = { images: nextConfig.images ?? undefined }) {
  return {
    exists: (path: string) => (path in changes ? changes[path] !== null : existsSync(join(WEB, path))),
    read: (path: string) => {
      const changed = changes[path];
      if (changed === null) throw new Error(`ENOENT: ${path}`);
      return changed ?? readFileSync(join(WEB, path), "utf8");
    },
    nextConfig: config,
  };
}

const real = (path: string) => readFileSync(join(WEB, path), "utf8");
const wrangler = () => real("wrangler.jsonc");
const only = (fragment: string) => [expect.stringContaining(fragment)];

describe("web/ runs on Cloudflare Workers through OpenNext, and nowhere else (DEC-823)", () => {
  it.skip("pending E11-9: finds nothing to fix in this tree", () => {
    expect(hostingProblems(tree())).toEqual([]);
    expect(existsSync(join(WEB, "..", "vercel.json"))).toBe(false);
    expect(existsSync(join(WEB, "src", "proxy.ts"))).toBe(false);
    expect(nextConfig.images).toEqual({ unoptimized: true });
  });

  it.skip("pending E11-9: flags a Vercel configuration left at the repository root", () => {
    expect(hostingProblems(tree({ "../vercel.json": '{ "git": {} }' }))).toEqual(only("vercel.json"));
  });

  it.skip("pending E11-9: flags images that the Worker would have to optimise", () => {
    expect(hostingProblems(tree({}, {}))).toEqual(only("images.unoptimized"));
    expect(hostingProblems(tree({}, { images: {} }))).toEqual(only("images.unoptimized"));
    expect(hostingProblems(tree({}, { images: { unoptimized: false } }))).toEqual(only("images.unoptimized"));
  });

  it.skip("pending E11-9: flags a Node proxy, a missing edge middleware, and a middleware that needs Node", () => {
    expect(hostingProblems(tree({ "src/proxy.ts": "export function proxy() {}" }))).toEqual(only("src/proxy.ts"));
    expect(hostingProblems(tree({ "src/middleware.ts": null }))).toEqual(only("src/middleware.ts"));
    const edge = real("src/middleware.ts");
    expect(hostingProblems(tree({ "src/middleware.ts": `import { readFile } from "node:fs/promises";\n${edge}` }))).toEqual(only("node:"));
    expect(hostingProblems(tree({ "src/middleware.ts": `${edge}\nexport const runtime = "nodejs";\n` }))).toEqual(only("runtime"));
  });

  it.skip("pending E11-9: flags a wrangler.jsonc without the Worker's name, entry, minification, flags, assets or self-reference", () => {
    expect(hostingProblems(tree({ "wrangler.jsonc": null }))).toEqual(only("wrangler.jsonc"));
    const config = JSON.parse(wrangler().replace(/^\s*\/\/.*$/gm, ""));
    const broken = (change: (c: Record<string, unknown>) => void) => {
      const copy = structuredClone(config);
      change(copy);
      return hostingProblems(tree({ "wrangler.jsonc": JSON.stringify(copy, null, 2) }));
    };
    expect(broken((c) => (c.name = "web"))).toEqual(only("name"));
    expect(broken((c) => (c.main = "worker.js"))).toEqual(only("main"));
    expect(broken((c) => delete c.minify)).toEqual(only("minify"));
    expect(broken((c) => (c.compatibility_flags = ["nodejs_compat"]))).toEqual(only("global_fetch_strictly_public"));
    expect(broken((c) => (c.compatibility_flags = ["global_fetch_strictly_public"]))).toEqual(only("nodejs_compat"));
    expect(broken((c) => delete c.compatibility_date)).toEqual(only("compatibility_date"));
    expect(broken((c) => (c.assets = { directory: ".open-next/assets" }))).toEqual(only("ASSETS"));
    expect(broken((c) => (c.assets = { directory: "public", binding: "ASSETS" }))).toEqual(only(".open-next/assets"));
    expect(broken((c) => (c.services = []))).toEqual(only("WORKER_SELF_REFERENCE"));
    expect(broken((c) => (c.services = [{ binding: "WORKER_SELF_REFERENCE", service: "other" }]))).toEqual(only("WORKER_SELF_REFERENCE"));
    expect(config).toMatchObject({ name: "owlhead-web", main: ".open-next/worker.js", minify: true });
  });

  it.skip("pending E11-9: flags missing build scripts, ignores and adapter config", () => {
    const pkg = JSON.parse(real("package.json"));
    const withScripts = (scripts: Record<string, string>) => JSON.stringify({ ...pkg, scripts }, null, 2);
    const without = (name: string) => Object.fromEntries(Object.entries<string>(pkg.scripts).filter(([key]) => key !== name));
    expect(hostingProblems(tree({ "package.json": withScripts(without("cf:build")) }))).toEqual(only("cf:build"));
    expect(hostingProblems(tree({ "package.json": withScripts(without("cf:preview")) }))).toEqual(only("cf:preview"));
    expect(hostingProblems(tree({ ".gitignore": real(".gitignore").replace("/.open-next/", "") }))).toEqual(only(".open-next/"));
    expect(hostingProblems(tree({ ".gitignore": real(".gitignore").replace("/.wrangler/", "") }))).toEqual(only(".wrangler/"));
    expect(hostingProblems(tree({ "open-next.config.ts": null }))).toEqual(only("open-next.config.ts"));
    expect(hostingProblems(tree({ "open-next.config.ts": "export default {};\n" }))).toEqual(only("defineCloudflareConfig"));
  });

  it.skip("pending E11-9: flags a web workflow that does not build the Worker and check its size, or that holds a Cloudflare token", () => {
    const workflow = "../.github/workflows/web.yml";
    const yml = real(workflow);
    expect(hostingProblems(tree({ [workflow]: yml.replace(/npm run cf:build/g, "true") }))).toEqual(only("cf:build"));
    expect(hostingProblems(tree({ [workflow]: yml.replace(/wrangler deploy --dry-run/g, "true") }))).toEqual(only("--dry-run"));
    expect(hostingProblems(tree({ [workflow]: yml.replace(/scripts\/workers\.mjs size/g, "true") }))).toEqual(only("workers.mjs size"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n# \${{ secrets.CLOUDFLARE_API_TOKEN }}\n` }))).toEqual(only("CLOUDFLARE"));
    expect(yml).not.toMatch(/CLOUDFLARE_(API_TOKEN|ACCOUNT_ID)|secrets\./);
  });
});

describe("the Worker's size budget (DEC-731)", () => {
  it.skip("pending E11-9: measures every module the dry run wrote, raw and gzip, leaving out source maps and the README", () => {
    const out = mkdtempSync(join(tmpdir(), "worker-size-"));
    try {
      const worker = "export default { fetch() { return new Response('owlhead'); } };\n".repeat(400);
      const chunk = new Uint8Array(4096).map((_, i) => (i * 7919) % 251);
      writeFileSync(join(out, "worker.js"), worker);
      writeFileSync(join(out, "chunk.wasm"), chunk);
      writeFileSync(join(out, "worker.js.map"), "x".repeat(50_000));
      writeFileSync(join(out, "README.md"), "x".repeat(50_000));
      const raw = Buffer.byteLength(worker) + chunk.byteLength;
      const gzip = gzipSync(worker).byteLength + gzipSync(chunk).byteLength;
      const size = workerSize(out);
      expect(size.rawBytes).toBe(raw);
      expect(Math.abs(size.gzipBytes - gzip)).toBeLessThanOrEqual(64);
      expect(size.gzipBytes).toBeLessThan(raw);
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
  });

  it.skip("pending E11-9: passes a Worker at its budget and fails one byte over, naming the budget it broke", () => {
    expect(WORKER_BUDGET).toEqual({ gzipBytes: 3 * MIB, rawBytes: 10 * MIB });
    expect(sizeProblems({ rawBytes: 10 * MIB, gzipBytes: 3 * MIB })).toEqual([]);
    expect(sizeProblems({ rawBytes: 7 * MIB, gzipBytes: 2 * MIB })).toEqual([]);
    expect(sizeProblems({ rawBytes: 10 * MIB, gzipBytes: 3 * MIB + 1 })).toEqual(only("gzip"));
    expect(sizeProblems({ rawBytes: 10 * MIB + 1, gzipBytes: 3 * MIB })).toEqual(only("raw"));
    expect(sizeProblems({ rawBytes: 11 * MIB, gzipBytes: 4 * MIB })).toHaveLength(2);
    expect(sizeProblems({ rawBytes: 100, gzipBytes: 60 }, { rawBytes: 99, gzipBytes: 60 })).toEqual(only("raw"));
  });

  it.skip("pending E11-9: records the budget in DEC-731", () => {
    expect(sizeProblems({ rawBytes: 0, gzipBytes: 0 })).toEqual([]);
    const decision = readFileSync(join(WEB, "..", "docs", "project", "decisions", "DEC-731.md"), "utf8");
    expect(decision).toContain("| **Status** | Accepted |");
    expect(decision).toContain("3 MiB");
    expect(decision).toContain("10 MiB");
  });
});
