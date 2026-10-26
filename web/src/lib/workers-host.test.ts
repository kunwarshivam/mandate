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
  it("pending E11-9: finds nothing to fix in this tree", () => {
    expect(hostingProblems(tree())).toEqual([]);
    expect(existsSync(join(WEB, "src", "proxy.ts"))).toBe(false);
    expect(nextConfig.images).toEqual({ unoptimized: true });
  });

  it("pending E21-38: flags any Vercel configuration at the repository root, since Workers alone hosts the app (DEC-1090)", () => {
    expect(existsSync(join(WEB, "..", "vercel.json"))).toBe(false);
    expect(hostingProblems(tree({ "../vercel.json": null }))).toEqual([]);
    for (const text of ['{ "git": { "deploymentEnabled": { "**": false, "main": true } } }', '{ "git": {} }', "{}"]) {
      expect(hostingProblems(tree({ "../vercel.json": text })), text).toEqual(only("vercel.json"));
    }
  });

  it("pending E11-9: flags images that the Worker would have to optimise", () => {
    expect(hostingProblems(tree({}, {}))).toEqual(only("images.unoptimized"));
    expect(hostingProblems(tree({}, { images: {} }))).toEqual(only("images.unoptimized"));
    expect(hostingProblems(tree({}, { images: { unoptimized: false } }))).toEqual(only("images.unoptimized"));
  });

  it("pending E11-9: flags a Node proxy, a missing edge middleware, and a middleware that needs Node", () => {
    expect(hostingProblems(tree({ "src/proxy.ts": "export function proxy() {}" }))).toEqual(only("src/proxy.ts"));
    expect(hostingProblems(tree({ "src/middleware.ts": null }))).toEqual(only("src/middleware.ts"));
    const edge = real("src/middleware.ts");
    expect(hostingProblems(tree({ "src/middleware.ts": `import { readFile } from "node:fs/promises";\n${edge}` }))).toEqual(only("node:"));
    expect(hostingProblems(tree({ "src/middleware.ts": `${edge}\nexport const runtime = "nodejs";\n` }))).toEqual(only("runtime"));
  });

  it("pending E11-9: flags a wrangler.jsonc without the Worker's name, entry, minification, flags, assets or self-reference, or with workers.dev on", () => {
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

  it("pending E11-9: flags missing build scripts, ignores and adapter config", () => {
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

  it("pending E11-9: flags a web workflow that does not build the Worker and check its size, or that holds a Cloudflare token", () => {
    const workflow = "../.github/workflows/web.yml";
    const yml = real(workflow);
    expect(hostingProblems(tree({ [workflow]: yml.replace(/npm run cf:build/g, "true") }))).toEqual(only("cf:build"));
    expect(hostingProblems(tree({ [workflow]: yml.replace(/wrangler deploy --dry-run/g, "true") }))).toEqual(only("--dry-run"));
    expect(hostingProblems(tree({ [workflow]: yml.replace(/scripts\/workers\.mjs size/g, "true") }))).toEqual(only("workers.mjs size"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n# \${{ secrets.CLOUDFLARE_API_TOKEN }}\n` }))).toEqual([expect.stringContaining("CLOUDFLARE_"), expect.stringContaining("reads a secret")]);
    expect(hostingProblems(tree({ [workflow]: yml.replace("WRANGLER_SEND_METRICS:", "CLOUDFLARE_ACCOUNT_ID: abc123\n          WRANGLER_SEND_METRICS:") }))).toEqual(only("CLOUDFLARE"));
    expect(hostingProblems(tree({ [workflow]: yml.replace("WRANGLER_SEND_METRICS:", "SUPABASE_SECRET: ${{ secrets.SUPABASE_SECRET }}\n          WRANGLER_SEND_METRICS:") }))).toEqual(only("secret"));
    expect(hostingProblems(tree({ [workflow]: yml.replace("WRANGLER_SEND_METRICS:", "SUPABASE_URL: ${{ vars.NEXT_PUBLIC_SUPABASE_URL }}\n          WRANGLER_SEND_METRICS:") }))).toEqual(only("variable"));
    expect(hostingProblems(tree({ [workflow]: yml.replace("on:\n", "on:\n  push:\n") }))).toEqual(only("never on a push"));
    expect(yml).not.toMatch(/CLOUDFLARE_(API_TOKEN|ACCOUNT_ID)|secrets\./);
  });

  it("pending E11-9: flags a workflow that runs wrangler deploy or wrangler login, and passes one that only dry-runs", () => {
    expect(hostingProblems(tree())).toEqual([]);
    const workflow = "../.github/workflows/web.yml";
    const yml = real(workflow);
    expect(yml).toContain("wrangler deploy --dry-run");
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: node_modules/.bin/wrangler deploy --dry-run --outdir .wrangler/dry-run\n` }))).toEqual([]);
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy --dry-run --outdir .wrangler/dry-run\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: node_modules/.bin/wrangler deploy\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy --dry-run --outdir dist\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ "../.github/workflows/ci.yml": `${real("../.github/workflows/ci.yml")}\n      - run: npx wrangler@4 deploy\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy --minify\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler deploy --dry-run && npx wrangler deploy\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler login\n` }))).toEqual(only("Cloudflare command"));
    expect(yml).not.toMatch(/wrangler login/);
  });

  it("pending E11-9: flags an account ID, a secret-looking value or a vars token committed in wrangler.jsonc, and passes the clean one", () => {
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
    expect(planted((c) => (c.preview_urls = true))).toEqual(only("preview_urls"));
    expect(planted((c) => delete c.preview_urls)).toEqual(only("preview_urls"));
    expect(planted((c) => (c.build = { command: "npm run cf:build" }))).toEqual(only("build must not be set"));
    expect(planted((c) => (c.routes = [{ pattern: "app.owlhead.ai", custom_domain: true }]))).toEqual(only("routes"));
    expect(hostingProblems(tree({ "wrangler.jsonc": `${text}\n// account_id: 0123456789abcdef0123456789abcdef\n` }))).toEqual(only("account_id"));
    expect(config).not.toHaveProperty("account_id");
    expect(config).not.toHaveProperty("vars");
  });
});

const DEPLOY = "../.github/workflows/web-deploy.yml";
const deployYml = () => real(DEPLOY);
const deployWith = (change: (yml: string) => string) => hostingProblems(tree({ [DEPLOY]: change(deployYml()) }));
const BUILD_ENV = "          NEXT_PUBLIC_SUPABASE_URL: ${{ vars.NEXT_PUBLIC_SUPABASE_URL }}\n";
const DEPLOY_ENV = "          CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}\n";

describe("only the deploy job deploys, from main, through the production environment (E21-38, DEC-1005, ES-12)", () => {
  it("pending E21-38: passes this tree's deploy workflow, and flags it missing", () => {
    expect(hostingProblems(tree())).toEqual([]);
    expect(hostingProblems(tree({ [DEPLOY]: null }))).toEqual(only("web-deploy.yml: missing"));
    const yml = deployYml();
    expect(yml).toContain("    environment: production\n");
    expect(yml).toContain("    if: github.ref == 'refs/heads/main' && vars.WEB_DEPLOY_ENABLED == 'true'\n");
    expect(yml.match(/\bsecrets\.[A-Z_]+/g)).toEqual(["secrets.CLOUDFLARE_API_TOKEN"]);
    for (const name of ["NEXT_PUBLIC_SUPABASE_URL", "NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY", "NEXT_PUBLIC_OWLHEAD_EMAIL_SIGNIN"]) {
      expect(yml, name).toContain(`          ${name}: \${{ vars.${name} }}\n`);
    }
    expect(yml).toContain('          NEXT_PUBLIC_OWLHEAD_SITE_ORIGIN: "https://owlhead.ai"\n');
    expect(yml).toContain('          NEXT_PUBLIC_OWLHEAD_APP_ORIGIN: "https://app.owlhead.ai"\n');
  });

  it("pending E21-38: flags any trigger but a push to main or a dispatch", () => {
    for (const change of [
      (y: string) => y.replace("  workflow_dispatch:\n", "  workflow_dispatch:\n  pull_request:\n"),
      (y: string) => y.replace("  workflow_dispatch:\n", "  workflow_dispatch:\n  pull_request_target:\n"),
      (y: string) => y.replace("  workflow_dispatch:\n", "  workflow_dispatch:\n  workflow_run:\n    workflows: [web]\n"),
      (y: string) => y.replace("  workflow_dispatch:\n", "  workflow_dispatch:\n  schedule:\n    - cron: \"0 0 * * *\"\n"),
      (y: string) => y.replace("    branches: [main]\n", "    branches: [main, \"claude/**\"]\n"),
      (y: string) => y.replace("    branches: [main]\n", ""),
      (y: string) => y.replace("    branches: [main]\n", "    tags: [\"*\"]\n"),
    ]) {
      expect(deployWith(change)).toEqual(only("push to main"));
    }
  });

  it("pending E21-38: flags a deploy job that may run off main, without the production environment, with more rights, or beside another job", () => {
    const gate = "    if: github.ref == 'refs/heads/main' && vars.WEB_DEPLOY_ENABLED == 'true'\n";
    expect(deployWith((y) => y.replace(gate, ""))).toEqual(only("refs/heads/main"));
    expect(deployWith((y) => y.replace(gate, "    if: always()\n"))).toEqual(only("refs/heads/main"));
    expect(deployWith((y) => y.replace(gate, "    if: github.ref == 'refs/heads/main'\n"))).toEqual(only("WEB_DEPLOY_ENABLED"));
    expect(deployWith((y) => y.replace(gate, "    if: vars.WEB_DEPLOY_ENABLED == 'true'\n"))).toEqual([expect.stringContaining("WEB_DEPLOY_ENABLED"), expect.stringContaining("never the whole job")]);
    expect(deployWith((y) => y.replace(gate, "    if: github.ref == 'refs/heads/main' || vars.WEB_DEPLOY_ENABLED == 'true'\n"))).toEqual([expect.stringContaining("WEB_DEPLOY_ENABLED"), expect.stringContaining("never the whole job")]);
    expect(deployWith((y) => y.replace("    environment: production\n", ""))).toEqual(only("production environment"));
    expect(deployWith((y) => y.replace("    environment: production\n", "    environment: staging\n"))).toEqual(only("production environment"));
    expect(deployWith((y) => y.replace("      contents: read\n", "      contents: write\n"))).toEqual(only("only read contents"));
    expect(deployWith((y) => y.replace("permissions: {}\n", "permissions: write-all\n"))).toEqual(only("permissions: {}"));
    expect(deployWith((y) => y.replace("          persist-credentials: false\n", ""))).toEqual(only("persist credentials"));
    expect(deployWith((y) => `${y}  smoke:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: true\n`)).toEqual(only("one job"));
    expect(deployWith((y) => y.replace("  deploy:\n", "  ship:\n"))).toEqual(only("one job"));
  });

  it("pending E21-38: lets only the last step deploy, after the build, reading the token and the account alone", () => {
    expect(deployWith((y) => y.replace("      - run: node_modules/.bin/wrangler deploy\n", "      - run: npx wrangler deploy\n"))).toEqual([expect.stringContaining("must end with"), expect.stringContaining("only the deploy step may read a secret"), expect.stringContaining("the build may read only"), expect.stringContaining("Cloudflare command")]);
    expect(deployWith((y) => y.replace("      - run: node_modules/.bin/wrangler deploy\n", "      - run: node_modules/.bin/wrangler deploy --env preview\n"))).toEqual([expect.stringContaining("must end with"), expect.stringContaining("only the deploy step may read a secret"), expect.stringContaining("the build may read only"), expect.stringContaining("Cloudflare command")]);
    expect(deployWith((y) => `${y}      - run: node scripts/workers.mjs size .wrangler/dry-run\n`)).toEqual(only("must end with"));
    expect(deployWith((y) => y.replace("      - run: npm run cf:build\n", "      - run: npm run build\n"))).toEqual(only("cf:build before"));
    expect(deployWith((y) => y.replace(BUILD_ENV, `${BUILD_ENV}          LEAK: \${{ secrets.CLOUDFLARE_API_TOKEN }}\n`))).toEqual(only("only the deploy step may read a secret"));
    expect(deployWith((y) => y.replace(BUILD_ENV, `${BUILD_ENV}          ACCOUNT: \${{ vars.CLOUDFLARE_ACCOUNT_ID }}\n`))).toEqual(only("the build may read only"));
    expect(deployWith((y) => y.replace(DEPLOY_ENV, `${DEPLOY_ENV}          R2: \${{ secrets.R2_SECRET_ACCESS_KEY }}\n`))).toEqual(only("only the deploy step may read a secret"));
    expect(deployWith((y) => y.replace(DEPLOY_ENV, `${DEPLOY_ENV}          ALL: \${{ toJSON(secrets) }}\n`))).toEqual(only("only the deploy step may read a secret"));
    for (const leak of ["Secrets.CLOUDFLARE_API_TOKEN", "SECRETS.CLOUDFLARE_API_TOKEN", "secrets['CLOUDFLARE_API_TOKEN']"]) {
      expect(deployWith((y) => y.replace(BUILD_ENV, `${BUILD_ENV}          LEAK: \${{ ${leak} }}\n`)), leak).toEqual(only("only the deploy step may read a secret"));
    }
    expect(deployWith((y) => y.replace(DEPLOY_ENV, `${DEPLOY_ENV}          KEY: \${{ vars.R2_ACCESS_KEY_ID }}\n`))).toEqual(only("the build may read only"));
    expect(deployWith((y) => y.replace('      WRANGLER_SEND_METRICS: "false"\n', '      WRANGLER_SEND_METRICS: "false"\n      CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}\n'))).toEqual(only("never the whole job"));
    expect(deployWith((y) => y.replace("      - run: npm ci --ignore-scripts\n", "      - run: npm ci --ignore-scripts\n      - run: npx wrangler deploy --minify\n"))).toEqual(only("Cloudflare command"));
    expect(deployWith((y) => `${y}      - run: npx wrangler login\n`)).toEqual([expect.stringContaining("must end with"), expect.stringContaining("Cloudflare command")]);
  });

  it("flags a deploy job that restores or saves a cache, or lets an install script run", () => {
    const setup = "          node-version-file: web/.nvmrc\n";
    expect(deployWith((y) => y.replace(setup, `${setup}          cache: npm\n`))).toEqual(only("no cache"));
    expect(deployWith((y) => y.replace(setup, `${setup}          cache-dependency-path: web/package-lock.json\n`))).toEqual(only("no cache"));
    expect(deployWith((y) => y.replace("      - run: npm ci --ignore-scripts\n", "      - uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v5\n        with:\n          path: ~/.npm\n          key: npm\n      - run: npm ci --ignore-scripts\n"))).toEqual(only("no cache"));
    expect(deployWith((y) => y.replace("npm ci --ignore-scripts", "npm ci"))).toEqual(only("--ignore-scripts"));
    expect(deployWith((y) => y.replace("      - run: npm ci --ignore-scripts\n", ""))).toEqual(only("--ignore-scripts"));
    expect(deployWith((y) => y.replace("      - run: npm ci --ignore-scripts\n", "      - run: npm ci --ignore-scripts\n      - run: npm install\n"))).toEqual(only("--ignore-scripts"));
  });

  it("pending E21-38: flags a secret or a variable read outside a step, a top-level env or defaults, and any other Cloudflare upload", () => {
    expect(deployWith((y) => y.replace("permissions: {}\n", "permissions: {}\n\nenv:\n  NEXT_PUBLIC_X: ${{ secrets.CLOUDFLARE_API_TOKEN }}\n"))).toEqual([expect.stringContaining("holds env at the top"), expect.stringContaining("never the whole job")]);
    expect(deployWith((y) => y.replace("permissions: {}\n", "permissions: {}\n\ndefaults:\n  run:\n    shell: bash\n"))).toEqual(only("holds defaults at the top"));
    expect(deployWith((y) => y.replace("  group: web-deploy\n", "  group: web-deploy-${{ vars.CLOUDFLARE_ACCOUNT_ID }}\n"))).toEqual(only("never the whole job"));
    const workflow = "../.github/workflows/web.yml";
    const yml = real(workflow);
    for (const step of [
      "npx wrangler -c wrangler.jsonc deploy",
      "npx wrangler pages deploy x",
      "npx wrangler deploy # --dry-run",
      "npx wrangler deploy --var X:--dry-run",
      "npx wrangler versions upload",
      "npx wrangler@4 --config wrangler.jsonc versions deploy",
      "npx wrangler deploy --dry-run && npx wrangler deploy",
      "npx opennextjs-cloudflare deploy",
      "npx wrangler \\\n          deploy",
      "npx wrangler deploy --dry-run false",
      "npx wrangler deploy --dry-run --no-dry-run",
      "npx wrangler deploy --dry-run=false --dry-run",
      'bash -c "npx wrangler deploy"',
      'npx wrangler "deploy"',
      "node node_modules/wrangler/bin/wrangler.js deploy",
      'echo "build #1" && npx wrangler deploy',
      "npx wrangler deploy --dry-run --outdir .wrangler/dry-run # and then deploy",
    ]) {
      expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: ${step}\n` })), step).toEqual(only("Cloudflare command"));
    }
    for (const step of ["uses: cloudflare/wrangler-action@v3", 'uses: "cloudflare/pages-action@v1"', "uses: someone/wrangler-action@v1"]) {
      expect(hostingProblems(tree({ [workflow]: `${yml}\n      - ${step}\n` })), step).toEqual(only("Cloudflare command"));
    }
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      - run: npx wrangler -c wrangler.jsonc login\n` }))).toEqual(only("Cloudflare command"));
    expect(hostingProblems(tree({ [workflow]: `${yml}\n      # npx wrangler deploy, in a comment\n` }))).toEqual([]);
    const pkg = JSON.parse(real("package.json"));
    for (const script of ["wrangler deploy", "node node_modules/wrangler/bin/wrangler.js deploy", "opennextjs-cloudflare build && opennextjs-cloudflare deploy"]) {
      expect(hostingProblems(tree({ "package.json": JSON.stringify({ ...pkg, scripts: { ...pkg.scripts, ship: script } }) })), script).toEqual(only("package.json: runs"));
    }
  });

  it("pending E21-38: flags any other workflow that names the production environment or a Cloudflare value", () => {
    const ci = "../.github/workflows/ci.yml";
    const yml = real(ci);
    expect(hostingProblems(tree({ [ci]: yml.replace(/^jobs:\n/m, "jobs:\n  leak:\n    environment: production\n    runs-on: ubuntu-24.04\n    steps:\n      - run: true\n") }))).toEqual(only("ci.yml: names the production environment"));
    expect(hostingProblems(tree({ [ci]: yml.replace(/^jobs:\n/m, "jobs:\n  leak:\n    environment:\n      name: production\n    runs-on: ubuntu-24.04\n    steps:\n      - run: true\n") }))).toEqual(only("ci.yml: names the production environment"));
    expect(hostingProblems(tree({ [ci]: yml.replace(/^jobs:\n/m, "jobs:\n  leak:\n    environment:\n      name: 'Production'\n    runs-on: ubuntu-24.04\n    steps:\n      - run: true\n") }))).toEqual(only("ci.yml: names the production environment"));
    expect(hostingProblems(tree({ [ci]: `${yml}\n# \${{ secrets.CLOUDFLARE_API_TOKEN }}\n` }))).toEqual(only("ci.yml: holds a CLOUDFLARE_"));
  });

  it("pending E21-38: lets r2-lockcheck.yml use the production environment only exactly as reviewed, and flags any edit to it (DEC-1091)", () => {
    expect(hostingProblems(tree())).toEqual([]);
    const r2 = "../.github/workflows/r2-lockcheck.yml";
    const yml = real(r2);
    for (const edit of [
      yml.replace("  workflow_dispatch:\n", "  workflow_dispatch:\n  pull_request:\n"),
      yml.replace("    if: github.ref == 'refs/heads/main'\n", ""),
      yml.replace("R2_SECRET_ACCESS_KEY: ${{ secrets.R2_SECRET_ACCESS_KEY }}", "R2_SECRET_ACCESS_KEY: ${{ secrets.R2_SECRET_ACCESS_KEY }}\n          LEAK: ${{ toJSON(secrets) }}"),
      `${yml}\n# a comment\n`,
    ]) {
      expect(edit).not.toBe(yml);
      expect(hostingProblems(tree({ [r2]: edit })), edit).toEqual(expect.arrayContaining([expect.stringContaining("r2-lockcheck.yml: differs from the reviewed")]));
    }
    expect(hostingProblems(tree({ [r2]: `${yml}      - run: npx wrangler deploy\n` }))).toEqual(expect.arrayContaining([expect.stringContaining("r2-lockcheck.yml: runs")]));
  });

  it("r2-lockcheck.yml builds the example with no secret and only runs the built binary beside R2's key and the token", () => {
    const yml = real("../.github/workflows/r2-lockcheck.yml");
    const steps = yml.split(/\n(?= {6}- )/);
    const secretSteps = steps.filter((step) => /\bsecrets\./.test(step));
    expect(secretSteps).toHaveLength(1);
    expect(secretSteps[0].trimEnd()).toMatch(/\n {8}run: target\/debug\/examples\/r2_finding$/);
    expect(secretSteps[0]).not.toMatch(/\bcargo\b/);
    const build = steps.findIndex((step) => step.includes("run: cargo build --locked --package mandate-objectstore --example r2_finding"));
    expect(build).toBeGreaterThan(-1);
    expect(steps[build]).not.toMatch(/\bsecrets\.|\benv:/);
    expect(build).toBeLessThan(steps.indexOf(secretSteps[0]));
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
  it("pending E11-9: refuses a dry run that left no directory, or no module in it, rather than measuring nothing", () => {
    expect(() => workerSize(join(tmpdir(), "owlhead-no-such-dry-run"))).toThrow(/no dry run output/);
    dryRun({}, (dir) => expect(() => workerSize(dir)).toThrow(/no Worker module/));
    dryRun(NO_MODULES, (dir) => expect(() => workerSize(dir)).toThrow(/no Worker module/));
    dryRun(SMALL, (dir) => expect(workerSize(dir).rawBytes).toBe(Buffer.byteLength(SMALL["worker.js"])));
  });

  it("pending E11-9: the size command passes a small Worker and fails one over budget, an empty dry run, and a bad command line", () => {
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

  it("pending E11-9: node scripts/workers.mjs size exits with the size command's status", () => {
    expect(dryRun(SMALL, (dir) => sizeCommand(["size", dir]).status)).toBe(0);
    const cli = (...args: string[]) => spawnSync(process.execPath, [join(WEB, "scripts", "workers.mjs"), ...args], { encoding: "utf8" }).status;
    expect(dryRun(SMALL, (dir) => cli("size", dir))).toBe(0);
    expect(dryRun(NO_MODULES, (dir) => cli("size", dir))).toBe(1);
    expect(dryRun({ "worker.js": randomBytes(WORKER_BUDGET.gzipBytes + 64 * 1024) }, (dir) => cli("size", dir))).toBe(1);
    expect(cli()).toBe(2);
  });

  it("pending E11-9: measures every module the dry run wrote, raw and gzip, leaving out source maps and the README", () => {
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

  it("pending E11-9: passes a Worker at its budget and fails one byte over, naming the budget it broke", () => {
    expect(WORKER_BUDGET).toEqual({ gzipBytes: 3 * MIB, rawBytes: 10 * MIB });
    expect(sizeProblems({ rawBytes: 10 * MIB, gzipBytes: 3 * MIB })).toEqual([]);
    expect(sizeProblems({ rawBytes: 7 * MIB, gzipBytes: 2 * MIB })).toEqual([]);
    expect(sizeProblems({ rawBytes: 10 * MIB, gzipBytes: 3 * MIB + 1 })).toEqual(only("gzip"));
    expect(sizeProblems({ rawBytes: 10 * MIB + 1, gzipBytes: 3 * MIB })).toEqual(only("raw"));
    expect(sizeProblems({ rawBytes: 11 * MIB, gzipBytes: 4 * MIB })).toHaveLength(2);
    expect(sizeProblems({ rawBytes: 100, gzipBytes: 60 }, { rawBytes: 99, gzipBytes: 60 })).toEqual(only("raw"));
  });

  it("pending E11-9: records the budget in DEC-731", () => {
    expect(sizeProblems({ rawBytes: 0, gzipBytes: 0 })).toEqual([]);
    const decision = readFileSync(join(WEB, "..", "docs", "project", "decisions", "DEC-731.md"), "utf8");
    expect(decision).toContain("| **Status** | Accepted |");
    expect(decision).toContain("3 MiB");
    expect(decision).toContain("10 MiB");
  });
});
