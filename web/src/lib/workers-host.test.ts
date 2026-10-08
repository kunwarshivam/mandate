// @vitest-environment node
import { spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import nextConfig from "../../next.config";
import { WORKER_BUDGET, hostingProblems, sizeCommand, sizeProblems, workerSize } from "../../scripts/workers.mjs";

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

  it.skip("pending E11-9: flags a wrangler.jsonc without the Worker's name, entry, minification, flags, assets or self-reference, or with workers.dev on", () => {
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
    expect(broken((c) => (c.workers_dev = true))).toEqual(only("workers_dev"));
    expect(broken((c) => delete c.workers_dev)).toEqual(only("workers_dev"));
    expect(broken((c) => (c.services = []))).toEqual(only("WORKER_SELF_REFERENCE"));
    expect(broken((c) => (c.services = [{ binding: "WORKER_SELF_REFERENCE", service: "other" }]))).toEqual(only("WORKER_SELF_REFERENCE"));
    expect(config).toMatchObject({ name: "owlhead-web", main: ".open-next/worker.js", minify: true, workers_dev: false });
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
    expect(hostingProblems(tree({ [workflow]: yml.replace("WRANGLER_SEND_METRICS:", "CLOUDFLARE_ACCOUNT_ID: abc123\n          WRANGLER_SEND_METRICS:") }))).toEqual(only("CLOUDFLARE"));
    expect(hostingProblems(tree({ [workflow]: yml.replace("WRANGLER_SEND_METRICS:", "SUPABASE_SECRET: ${{ secrets.SUPABASE_SECRET }}\n          WRANGLER_SEND_METRICS:") }))).toEqual(only("secret"));
    expect(yml).not.toMatch(/CLOUDFLARE_(API_TOKEN|ACCOUNT_ID)|secrets\./);
  });

  it.skip("pending E11-9: flags a workflow that runs wrangler deploy or wrangler login, and passes one that only dry-runs", () => {
    expect(hostingProblems(tree())).toEqual([]);
    const workflow = "../.github/workflows/web.yml";
    const yml = real(workflow);
    expect(yml).toContain("wrangler deploy --dry-run");
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy --dry-run --outdir dist\n` }))).toEqual([]);
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy\n` }))).toEqual(only("wrangler deploy"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy --minify\n` }))).toEqual(only("wrangler deploy"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy --dry-run && npx wrangler deploy\n` }))).toEqual(only("wrangler deploy"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler login\n` }))).toEqual(only("wrangler login"));
    expect(yml).not.toMatch(/wrangler login/);
  });

  it.skip("pending E11-9: flags an account ID, a secret-looking value or a vars token committed in wrangler.jsonc, and passes the clean one", () => {
    expect(hostingProblems(tree())).toEqual([]);
    const text = wrangler();
    expect(hostingProblems(tree({ "wrangler.jsonc": text }))).toEqual([]);
    const config = JSON.parse(text.replace(/^\s*\/\/.*$/gm, ""));
    const planted = (change: (c: Record<string, unknown>) => void) => {
      const copy = structuredClone(config);
      change(copy);
      return hostingProblems(tree({ "wrangler.jsonc": JSON.stringify(copy, null, 2) }));
    };
    expect(planted((c) => (c.account_id = "0123456789abcdef0123456789abcdef"))).toEqual(only("account_id"));
    expect(planted((c) => (c.upload_key = `sb_secret_${"x".repeat(32)}`))).toEqual(only("secret"));
    expect(planted((c) => (c.vars = { CLOUDFLARE_API_TOKEN: "y".repeat(40) }))).toEqual(only("vars"));
    expect(hostingProblems(tree({ "wrangler.jsonc": `${text}\n// account_id: 0123456789abcdef0123456789abcdef\n` }))).toEqual(only("account_id"));
    expect(config).not.toHaveProperty("account_id");
    expect(config).not.toHaveProperty("vars");
  });
});

/** A dry run's output directory holding `files`, removed after `check`. */
function dryRun<T>(files: Record<string, string | Uint8Array>, check: (dir: string) => T): T {
  const dir = mkdtempSync(join(tmpdir(), "worker-dry-run-"));
  try {
    for (const [name, content] of Object.entries(files)) writeFileSync(join(dir, name), content);
    return check(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const SMALL = { "worker.js": "export default {};\n", "README.md": "dry run", "worker.js.map": "{}" };
const NO_MODULES = { "README.md": "dry run", "worker.js.map": "{}" };

describe("the Worker's size budget (DEC-731)", () => {
  it.skip("pending E11-9: refuses a dry run that left no directory, or no module in it, rather than measuring nothing", () => {
    expect(() => workerSize(join(tmpdir(), "owlhead-no-such-dry-run"))).toThrow(/no dry run output/);
    dryRun({}, (dir) => expect(() => workerSize(dir)).toThrow(/no Worker module/));
    dryRun(NO_MODULES, (dir) => expect(() => workerSize(dir)).toThrow(/no Worker module/));
    dryRun(SMALL, (dir) => expect(workerSize(dir).rawBytes).toBe(Buffer.byteLength(SMALL["worker.js"])));
  });

  it.skip("pending E11-9: the size command passes a small Worker and fails one over budget, an empty dry run, and a bad command line", () => {
    expect(dryRun(SMALL, (dir) => sizeCommand(["size", dir]).status)).toBe(0);
    const over = { "worker.js": randomBytes(WORKER_BUDGET.gzipBytes + 64 * 1024) };
    const overBudget = dryRun(over, (dir) => sizeCommand(["size", dir]));
    expect(overBudget.status).toBe(1);
    expect(overBudget.lines.join("\n")).toContain("gzip budget");
    expect(dryRun(NO_MODULES, (dir) => sizeCommand(["size", dir]).status)).toBe(1);
    expect(dryRun({}, (dir) => sizeCommand(["size", dir]).status)).toBe(1);
    expect(sizeCommand(["size", join(tmpdir(), "owlhead-no-such-dry-run")]).status).toBe(1);
    expect(sizeCommand([]).status).toBe(2);
    expect(sizeCommand(["size"]).status).toBe(2);
    expect(sizeCommand(["measure", "."]).status).toBe(2);
  });

  it.skip("pending E11-9: node scripts/workers.mjs size exits with the size command's status", () => {
    expect(dryRun(SMALL, (dir) => sizeCommand(["size", dir]).status)).toBe(0);
    const cli = (...args: string[]) => spawnSync(process.execPath, [join(WEB, "scripts", "workers.mjs"), ...args], { encoding: "utf8" }).status;
    expect(dryRun(SMALL, (dir) => cli("size", dir))).toBe(0);
    expect(dryRun(NO_MODULES, (dir) => cli("size", dir))).toBe(1);
    expect(dryRun({ "worker.js": randomBytes(WORKER_BUDGET.gzipBytes + 64 * 1024) }, (dir) => cli("size", dir))).toBe(1);
    expect(cli()).toBe(2);
  });

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
