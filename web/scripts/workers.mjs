// The Cloudflare Workers host (DEC-823, DEC-731). CI builds the Worker with OpenNext (`npm run cf:build`),
// has Wrangler write the bundle it would upload (`wrangler deploy --dry-run --outdir`, which needs no
// Cloudflare account or token), and runs `node scripts/workers.mjs size <outdir>`, which fails when the
// Worker is over its budget, or when the dry run left no Worker to measure. `hostingProblems` lists what would stop the app from running on Workers,
// or leave a second host configured; the unit tests run it against this tree.

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

/** The Worker's size budget (DEC-731 item 6): Workers Free's 3 MiB gzip cap, under the paid plan's 10 MiB. */
export const WORKER_BUDGET = { gzipBytes: 3 * 1024 * 1024, rawBytes: 10 * 1024 * 1024 };

/**
 * The raw and gzip size of the Worker's modules in a dry run's output directory: every file but source
 * maps and Wrangler's README. Throws when the directory is missing or holds no module, so a failed dry
 * run never passes as a small Worker.
 * @param {string} outdir
 * @returns {{ rawBytes: number, gzipBytes: number }}
 */
export function workerSize(outdir) {
  if (!existsSync(outdir) || !statSync(outdir).isDirectory()) throw new Error(`no dry run output at ${outdir}`);
  const modules = readdirSync(outdir, { recursive: true })
    .map((name) => join(outdir, String(name)))
    .filter((path) => statSync(path).isFile() && !path.endsWith(".map") && basename(path) !== "README.md");
  if (modules.length === 0) throw new Error(`no Worker module in ${outdir}`);
  let rawBytes = 0;
  let gzipBytes = 0;
  for (const path of modules) {
    const content = readFileSync(path);
    rawBytes += content.byteLength;
    gzipBytes += gzipSync(content).byteLength;
  }
  return { rawBytes, gzipBytes };
}

const mib = (bytes) => `${(bytes / (1024 * 1024)).toFixed(2)} MiB`;

/**
 * One line for each budget the Worker exceeds; none when it fits.
 * @param {{ rawBytes: number, gzipBytes: number }} size
 * @param {{ rawBytes: number, gzipBytes: number }} [budget]
 * @returns {string[]}
 */
export function sizeProblems(size, budget = WORKER_BUDGET) {
  const problems = [];
  if (size.gzipBytes > budget.gzipBytes) problems.push(`the Worker is ${size.gzipBytes} bytes gzipped, over its gzip budget of ${budget.gzipBytes} (DEC-731 item 6)`);
  if (size.rawBytes > budget.rawBytes) problems.push(`the Worker is ${size.rawBytes} bytes uncompressed, over its raw budget of ${budget.rawBytes} (DEC-731 item 6)`);
  return problems;
}

/**
 * `node scripts/workers.mjs size <outdir>`, without exiting: status 0 when the Worker fits its budget,
 * 1 when it does not or there is no Worker to measure, 2 for a bad command line; one line each to print.
 * @param {string[]} args
 * @returns {{ status: number, lines: string[] }}
 */
export function sizeCommand(args) {
  if (args.length !== 2 || args[0] !== "size") return { status: 2, lines: ["usage: node scripts/workers.mjs size <dry-run outdir>"] };
  let size;
  try {
    size = workerSize(args[1]);
  } catch (error) {
    return { status: 1, lines: [error instanceof Error ? error.message : String(error)] };
  }
  const problems = sizeProblems(size);
  const summary = `Worker: ${mib(size.rawBytes)} raw of ${mib(WORKER_BUDGET.rawBytes)}, ${mib(size.gzipBytes)} gzip of ${mib(WORKER_BUDGET.gzipBytes)}`;
  return { status: problems.length === 0 ? 0 : 1, lines: [summary, ...problems] };
}

/**
 * What stops `web/` from running on Workers through OpenNext, one line each.
 * @param {{ exists(path: string): boolean, read(path: string): string, nextConfig: { images?: { unoptimized?: boolean } } }} tree
 *   paths relative to `web/`
 * @returns {string[]}
 */
export function hostingProblems(tree) {
  const problems = [];
  if (tree.exists("../vercel.json")) problems.push("../vercel.json: remove the Vercel configuration; the app is hosted on Workers alone (DEC-823 item 2)");
  if (tree.nextConfig.images?.unoptimized !== true) problems.push("next.config.ts: set images.unoptimized, so the Worker never optimises images (DEC-731 item 2)");
  problems.push(...middlewareProblems(tree), ...wranglerProblems(tree), ...buildProblems(tree), ...workflowProblems(tree));
  return problems;
}

function middlewareProblems(tree) {
  if (tree.exists("src/proxy.ts")) return ["src/proxy.ts: OpenNext cannot run Node's proxy; the edge src/middleware.ts replaces it (DEC-731 item 1)"];
  if (!tree.exists("src/middleware.ts")) return ["src/middleware.ts: missing; the edge middleware is the app's auth gate (DEC-731 item 1)"];
  const source = tree.read("src/middleware.ts");
  const problems = [];
  if (/(from\s*|import\s*\(\s*|require\s*\(\s*)["']node:/.test(source)) problems.push("src/middleware.ts imports a node: module, which edge middleware cannot load (DEC-731 item 1)");
  if (/export\s+const\s+runtime\b/.test(source)) problems.push("src/middleware.ts sets a runtime; edge middleware keeps Next's edge default (DEC-731 item 1)");
  return problems;
}

const WORKER = "owlhead-web";
const SECRET_LOOKING = /secret|service_role|private_key|-----BEGIN|\beyJ[\w-]{8,}\./i;

function wranglerProblems(tree) {
  if (!tree.exists("wrangler.jsonc")) return ["wrangler.jsonc: missing; it configures the Worker (DEC-731 item 3)"];
  const text = tree.read("wrangler.jsonc");
  const config = JSON.parse(text.replace(/^\s*\/\/.*$/gm, ""));
  const flags = Array.isArray(config.compatibility_flags) ? config.compatibility_flags : [];
  const services = Array.isArray(config.services) ? config.services : [];
  const { vars, ...rest } = config;
  const checks = [
    [config.name === WORKER, `name must be "${WORKER}"`],
    [config.main === ".open-next/worker.js", 'main must be ".open-next/worker.js", the worker OpenNext builds'],
    [config.minify === true, "minify must be true, to keep the Worker under its size budget"],
    [flags.includes("nodejs_compat"), "compatibility_flags must include nodejs_compat"],
    [flags.includes("global_fetch_strictly_public"), "compatibility_flags must include global_fetch_strictly_public"],
    [/^\d{4}-\d{2}-\d{2}$/.test(String(config.compatibility_date ?? "")), "compatibility_date must be set"],
    [config.assets?.binding === "ASSETS", 'assets.binding must be "ASSETS"'],
    [config.assets?.directory === ".open-next/assets", 'assets.directory must be ".open-next/assets"'],
    [config.workers_dev === false, "workers_dev must be false, so only the custom domain serves the app (DEC-823 item 2)"],
    [services.some((s) => s?.binding === "WORKER_SELF_REFERENCE" && s?.service === WORKER), `services must bind WORKER_SELF_REFERENCE to "${WORKER}"`],
    [!/account_id/.test(text), "account_id must not be committed; the founder's dashboard holds the account (DEC-823 item 4)"],
    [vars === undefined, "vars must not be committed; the founder sets the public variables in the dashboard (DEC-823 items 4 and 5)"],
    [!SECRET_LOOKING.test(JSON.stringify(rest)), "holds a secret-looking value; no credential goes in the Worker's configuration (DEC-823 item 4)"],
  ];
  return checks.filter(([ok]) => !ok).map(([, message]) => `wrangler.jsonc: ${message}`);
}

function buildProblems(tree) {
  const problems = [];
  const scripts = JSON.parse(tree.read("package.json")).scripts ?? {};
  if (!/opennextjs-cloudflare build/.test(scripts["cf:build"] ?? "")) problems.push('package.json: the "cf:build" script must run opennextjs-cloudflare build');
  if (!/opennextjs-cloudflare preview/.test(scripts["cf:preview"] ?? "")) problems.push('package.json: the "cf:preview" script must run opennextjs-cloudflare preview');
  const ignored = tree.read(".gitignore").split("\n").map((line) => line.trim());
  for (const dir of [".open-next/", ".wrangler/"]) {
    if (!ignored.includes(`/${dir}`) && !ignored.includes(dir)) problems.push(`.gitignore: ignore ${dir}, the build's output`);
  }
  if (!tree.exists("open-next.config.ts")) problems.push("open-next.config.ts: missing; it configures the adapter");
  else if (!/defineCloudflareConfig\(/.test(tree.read("open-next.config.ts"))) problems.push("open-next.config.ts must export defineCloudflareConfig(...)");
  return problems;
}

const WEB_WORKFLOW = "../.github/workflows/web.yml";
const WORKFLOWS = join(dirname(fileURLToPath(import.meta.url)), "..", "..", ".github", "workflows");
const DEPLOY = /\b(wrangler(?:@[\w.^~-]+)?\s+(?:deploy|publish|versions\s+(?:upload|deploy))|opennextjs-cloudflare(?:@[\w.^~-]+)?\s+(?:deploy|upload))\b([^\n;&|]*)/g;

/** One line naming the first deploy or login in `text` that is not a dry run, or none. */
function deployProblems(path, text) {
  const problems = [];
  const deploys = [...text.matchAll(DEPLOY)].filter((match) => !match[0].startsWith("wrangler deploy") || !/--dry-run\b/.test(match[2]));
  if (deploys.length > 0) problems.push(`${path}: runs ${deploys[0][1]} (wrangler deploy without --dry-run); only Cloudflare's own build deploys (DEC-823 item 4)`);
  if (/\bwrangler(?:@[\w.^~-]+)?\s+login\b/.test(text)) problems.push(`${path}: runs wrangler login; no Cloudflare credential exists outside the founder's account (DEC-823 item 4)`);
  return problems;
}

function workflowProblems(tree) {
  const workflows = existsSync(WORKFLOWS) ? readdirSync(WORKFLOWS).filter((name) => /\.ya?ml$/.test(name)).map((name) => `../.github/workflows/${name}`) : [];
  const problems = [];
  for (const path of new Set([WEB_WORKFLOW, ...workflows])) {
    if (tree.exists(path)) problems.push(...deployProblems(path, tree.read(path)));
  }
  problems.push(...deployProblems("package.json", JSON.stringify(JSON.parse(tree.read("package.json")).scripts ?? {})));
  if (!tree.exists(WEB_WORKFLOW)) return [...problems, `${WEB_WORKFLOW}: missing`];
  const yml = tree.read(WEB_WORKFLOW);
  if (!/npm run cf:build/.test(yml)) problems.push(`${WEB_WORKFLOW}: must build the Worker with npm run cf:build`);
  if (!/wrangler deploy --dry-run --outdir/.test(yml)) problems.push(`${WEB_WORKFLOW}: must bundle the Worker with wrangler deploy --dry-run --outdir`);
  if (!/node scripts\/workers\.mjs size/.test(yml)) problems.push(`${WEB_WORKFLOW}: must check the Worker's budget with node scripts/workers.mjs size`);
  if (/CLOUDFLARE_|secrets\./.test(yml)) problems.push(`${WEB_WORKFLOW}: holds a CLOUDFLARE_ value or reads secrets.; CI only builds, with no credential (DEC-823 item 4)`);
  return problems;
}

const invokedAsScript = process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  const { status, lines } = sizeCommand(process.argv.slice(2));
  for (const line of lines) (status === 0 ? console.log : console.error)(line);
  process.exitCode = status;
}
