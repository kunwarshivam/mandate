// The Cloudflare Workers host (DEC-823, DEC-731, DEC-1005). CI builds the Worker with OpenNext (`npm run cf:build`),
// has Wrangler write the bundle it would upload (`wrangler deploy --dry-run --outdir`, which needs no
// Cloudflare account or token), and runs `node scripts/workers.mjs size <outdir>`, which fails when the
// Worker is over its budget, or when the dry run left no Worker to measure; then `node scripts/workers.mjs smoke`
// boots that Worker under `wrangler dev` and fails unless the public site and the app both answer `/` 200. `hostingProblems` lists what would stop the app from running on Workers,
// leave a second host deploying, or let anything but the deploy job of `web-deploy.yml` on `main` deploy or
// reach the Cloudflare token; the unit tests run it against this tree.

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { request } from "node:http";
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
 * What the pre-deploy smoke asks the built Worker (2026-10-29's outage, when every request the Next
 * server handled threw while static files and the host redirects still answered): the public site's
 * root and the app's, each by its own host.
 */
export const SMOKE_REQUESTS = [
  { host: "owlhead.ai", path: "/" },
  { host: "app.owlhead.ai", path: "/" },
];

/** The runtime variables the smoke runs the Worker with: the dashboard's data source (DEC-823, DEC-975), with an API that answers nothing. */
const SMOKE_VARS = ["OWLHEAD_DATA_SOURCE:api", "OWLHEAD_API_ORIGIN:https://api.invalid"];
const SMOKE_READY_MS = 90_000;
const SMOKE_REQUEST_MS = 30_000;

/**
 * Each of `SMOKE_REQUESTS` that was not answered 200, one line each, with the first error line the
 * Worker answered, never its stack.
 * @param {{ host: string, path: string, status: number, body: string }[]} answers
 * @returns {string[]}
 */
export function smokeProblems(answers) {
  return SMOKE_REQUESTS.flatMap(({ host, path }) => {
    const answer = answers.find((a) => a.host === host && a.path === path);
    if (!answer) return [`${host}${path} was not asked`];
    if (answer.status === 200) return [];
    const said = (answer.body.split("\n").find((line) => /\S/.test(line)) ?? "").trim().slice(0, 240);
    return [`${host}${path} answered ${answer.status}${said ? `: ${said}` : ""}`];
  });
}

/**
 * Starts the built Worker (`.open-next/`, as `wrangler.jsonc` names it) under `wrangler dev`, asks it
 * each of `SMOKE_REQUESTS` by its host, and stops it, its `workerd` children included. Needs no
 * Cloudflare account or token: `wrangler dev` runs locally.
 * @returns {Promise<{ host: string, path: string, status: number, body: string }[]>}
 */
export async function serveWorker() {
  const port = 8700 + Math.floor(Math.random() * 200);
  const args = ["dev", "--port", String(port), "--ip", "127.0.0.1", ...SMOKE_VARS.flatMap((v) => ["--var", v])];
  const child = spawn(join("node_modules", ".bin", "wrangler"), args, { detached: true, env: { ...process.env, WRANGLER_SEND_METRICS: "false" }, stdio: ["ignore", "pipe", "pipe"] });
  const stop = () => {
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {
      child.kill("SIGTERM");
    }
  };
  try {
    await new Promise((ready, refuse) => {
      let seen = "";
      const timer = setTimeout(() => refuse(new Error(`wrangler dev was not ready within ${SMOKE_READY_MS / 1000} s: ${seen.slice(-400)}`)), SMOKE_READY_MS);
      const watch = (chunk) => {
        seen += String(chunk);
        if (/Ready on http/.test(seen)) {
          clearTimeout(timer);
          ready(undefined);
        }
      };
      child.stdout.on("data", watch);
      child.stderr.on("data", watch);
      child.on("exit", (code) => {
        clearTimeout(timer);
        refuse(new Error(`wrangler dev exited ${code} before it was ready: ${seen.slice(-400)}`));
      });
    });
    const ask = ({ host, path }) =>
      new Promise((answer, refuse) => {
        const sent = request({ host: "127.0.0.1", port, path, method: "GET", headers: { host }, timeout: SMOKE_REQUEST_MS }, (response) => {
          let body = "";
          response.setEncoding("utf8");
          response.on("data", (chunk) => (body += chunk));
          response.on("end", () => answer({ host, path, status: response.statusCode ?? 0, body }));
        });
        sent.on("timeout", () => sent.destroy(new Error(`${host}${path} did not answer within ${SMOKE_REQUEST_MS / 1000} s`)));
        sent.on("error", refuse);
        sent.end();
      });
    const answers = [];
    for (const asked of SMOKE_REQUESTS) answers.push(await ask(asked));
    return answers;
  } finally {
    stop();
  }
}

/**
 * `node scripts/workers.mjs smoke`, without exiting: status 0 when the built Worker answers each of
 * `SMOKE_REQUESTS` 200, 1 when it does not or never starts, 2 for a bad command line; one line each.
 * @param {string[]} args
 * @param {() => Promise<{ host: string, path: string, status: number, body: string }[]>} serve
 * @returns {Promise<{ status: number, lines: string[] }>}
 */
export async function smokeCommand(args, serve = serveWorker) {
  if (args.length !== 1 || args[0] !== "smoke") return { status: 2, lines: ["usage: node scripts/workers.mjs smoke"] };
  let answers;
  try {
    answers = await serve();
  } catch (error) {
    return { status: 1, lines: [error instanceof Error ? error.message : String(error)] };
  }
  const problems = smokeProblems(answers);
  const summary = `Worker smoke: ${answers.map((a) => `${a.host}${a.path} ${a.status}`).join(", ")}`;
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
  problems.push(...vercelProblems(tree));
  if (tree.nextConfig.images?.unoptimized !== true) problems.push("next.config.ts: set images.unoptimized, so the Worker never optimises images (DEC-731 item 2)");
  problems.push(...middlewareProblems(tree), ...wranglerProblems(tree), ...buildProblems(tree), ...workflowProblems(tree));
  return problems;
}

/** Workers alone hosts the app, so no Vercel configuration may come back (DEC-1090; DEC-823 item 3). */
function vercelProblems(tree) {
  return tree.exists("../vercel.json") ? ["../vercel.json: remove it; Cloudflare Workers alone hosts the app (DEC-1090)"] : [];
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
    [config.preview_urls === false, "preview_urls must be false, so no version gets its own address (DEC-823 item 2)"],
    [config.build === undefined, "build must not be set; the deploy step runs no repository command while it holds the token (DEC-1005 item 2)"],
    [config.routes === undefined && config.route === undefined, "routes must not be committed; the founder adds the custom domain in the dashboard (DEC-1005 item 5)"],
    [services.some((s) => s?.binding === "WORKER_SELF_REFERENCE" && s?.service === WORKER), `services must bind WORKER_SELF_REFERENCE to "${WORKER}"`],
    [!/account_id/.test(text), "account_id must not be committed; the founder's dashboard holds the account (DEC-823 item 4)"],
    [vars === undefined, "vars must not be committed; the founder sets the public variables in the dashboard (DEC-823 items 4 and 5)"],
    [config.keep_vars === true, "keep_vars must be true, so a deploy keeps the variables the founder set in the dashboard (DEC-823 item 5, DEC-975)"],
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
/**
 * The other workflows that may use the `production` environment, each pinned to the SHA-256 of the
 * text a reviewer read (DEC-1091): `r2-lockcheck.yml` (E21-40, #1454), dispatched from `main` only,
 * one job, R2's key and the token in its one step, which runs the example a step with no secret
 * built, so no build script runs beside them. Any edit fails until the pin is re-read and moved.
 */
const PINNED_PRODUCTION_WORKFLOWS = { "../.github/workflows/r2-lockcheck.yml": "e8774b31a9a984e4ee1f26e807b3fdc68004b356d03762bc283386a62784c059" };
const DEPLOY_WORKFLOW = "../.github/workflows/web-deploy.yml";
const WORKFLOWS = join(dirname(fileURLToPath(import.meta.url)), "..", "..", ".github", "workflows");
/** The deploy workflow's triggers, exactly: a push to `main` touching the app, or a dispatch (DEC-1005 item 1). */
const DEPLOY_TRIGGERS = ["on:", "  push:", "    branches: [main]", "    paths:", '      - "web/**"', '      - ".github/workflows/web-deploy.yml"', "  workflow_dispatch:"].join("\n");
/** The deploy job's condition, exactly: `main`, and the switch the founder sets once DEC-1005 is accepted (DEC-1005 item 7). */
const DEPLOY_IF = "    if: github.ref == 'refs/heads/main' && vars.WEB_DEPLOY_ENABLED == 'true'";
/** The one step that deploys, and the only secret and variables it may read (DEC-1005 items 2 and 3). */
const DEPLOY_STEP = { run: "node_modules/.bin/wrangler deploy", secrets: ["CLOUDFLARE_API_TOKEN"], vars: ["CLOUDFLARE_ACCOUNT_ID"] };
/** The deploy workflow's top-level keys: nothing that sets an environment or defaults for every job. */
const TOP_LEVEL = ["name", "on", "permissions", "concurrency", "jobs"];
/** The variables the build may read: public values only, which Next inlines into the bundle (DEC-823 item 5). */
const PUBLIC_VARS = /^NEXT_PUBLIC_[A-Z0-9_]+$/;

/**
 * What names a Cloudflare tool: Wrangler, the OpenNext adapter, or a Cloudflare action. The build's
 * own dry-run directory and Wrangler's metrics switch are the two names that run nothing.
 */
const CLOUDFLARE_TOOL = /wrangler|opennext|cloudflare\//i;
const HARMLESS = /\.wrangler\/dry-run\b|\bWRANGLER_SEND_METRICS\b/g;
/**
 * The only commands that may name a Cloudflare tool, exactly as written: the dry run and the Worker's build and preview scripts.
 * Wrangler runs from `node_modules/.bin`, where `npm ci` put the lockfile's version; `npx` would fetch one the lockfile does not hold.
 */
const ALLOWED_COMMANDS = ["node_modules/.bin/wrangler deploy --dry-run --outdir .wrangler/dry-run", "opennextjs-cloudflare build", "opennextjs-cloudflare build && opennextjs-cloudflare preview"];

/**
 * One line for the first line of `lines` that names a Cloudflare tool and is not an allowed command
 * exactly, or none. It is an allowlist, so a deploy hidden by flags, quotes, a path or a comment
 * fails rather than passes; it checks for drift, and the environment, not this check, holds the token.
 */
function deployProblems(path, lines) {
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith("#") || !CLOUDFLARE_TOOL.test(trimmed.replace(HARMLESS, ""))) continue;
    const command = trimmed.replace(/^(?:-\s+)?run:\s*/, "");
    if (!ALLOWED_COMMANDS.includes(command)) {
      return [`${path}: runs ${JSON.stringify(trimmed)}, a Cloudflare command other than the Worker's build and dry run; only the deploy job of ${DEPLOY_WORKFLOW} deploys (DEC-1005 item 1)`];
    }
  }
  return [];
}

/** The workflow's lines with comments and blank lines left out. */
const meaningful = (text) =>
  text
    .split("\n")
    .map((line) => line.replace(/\s+$/, ""))
    .filter((line) => line.trim() !== "" && !line.trimStart().startsWith("#"));

/** The lines of each top-level key's block, its own line first. */
function topBlocks(lines) {
  const blocks = new Map();
  let current = null;
  for (const line of lines) {
    if (!/^\s/.test(line)) {
      current = line.replace(/:.*$/, "");
      blocks.set(current, []);
    }
    blocks.get(current)?.push(line);
  }
  return blocks;
}

/** Splits `lines` before each line `starts` accepts; the lines before the first such line come first. */
function splitAt(lines, starts) {
  const parts = [[]];
  for (const line of lines) {
    if (starts(line)) parts.push([]);
    parts[parts.length - 1].push(line);
  }
  return parts;
}

/** The names each `secrets` and `vars` reference in `text` reads; a bare `secrets` or `vars` (as in `toJSON(secrets)`) reads `*`. GitHub's contexts ignore case, and so does this. */
function references(text, context) {
  return [...text.matchAll(new RegExp(`\\b${context}\\b(?:\\s*(?:\\.\\s*|\\[\\s*["'])([A-Za-z0-9_]+))?`, "gi"))].map((match) => match[1] ?? "*");
}

function deployWorkflowProblems(tree) {
  if (!tree.exists(DEPLOY_WORKFLOW)) return [`${DEPLOY_WORKFLOW}: missing; it is the one job that deploys main (DEC-1005 item 1)`];
  const lines = meaningful(tree.read(DEPLOY_WORKFLOW));
  const blocks = topBlocks(lines);
  const problems = [];
  const fail = (message) => problems.push(`${DEPLOY_WORKFLOW}: ${message}`);
  if ((blocks.get("on") ?? []).join("\n") !== DEPLOY_TRIGGERS) fail("must run only on a push to main touching web/ or itself, or on workflow_dispatch (DEC-1005 item 1)");
  if ((blocks.get("permissions") ?? []).join("\n") !== "permissions: {}") fail("must set permissions: {} at the top (DEC-1005 item 3)");
  const extra = [...blocks.keys()].filter((key) => !TOP_LEVEL.includes(key));
  if (extra.length > 0) fail(`holds ${extra.join(", ")} at the top; only ${TOP_LEVEL.join(", ")} may be there, so no secret reaches the whole workflow (DEC-1005 item 2)`);
  const [header, ...jobs] = splitAt((blocks.get("jobs") ?? []).slice(1), (line) => /^ {2}\S/.test(line));
  if (header.length > 0 || jobs.length !== 1 || jobs[0][0] !== "  deploy:") return [...problems, `${DEPLOY_WORKFLOW}: must hold one job, deploy (DEC-1005 item 1)`];
  const job = jobs[0];
  const [settings, ...steps] = splitAt(job.slice(1), (line) => /^ {6}- /.test(line));
  const has = (lines, wanted) => lines.includes(wanted);
  if (!has(settings, DEPLOY_IF)) fail("the deploy job must run only on refs/heads/main, and only once WEB_DEPLOY_ENABLED is true (DEC-1005 items 1 and 7)");
  if (!has(settings, "    environment: production")) fail("the deploy job must use the production environment, which holds the token (DEC-1005 item 2)");
  if (!settings.join("\n").includes("    permissions:\n      contents: read")) fail("the deploy job may only read contents (DEC-1005 item 3)");
  if (!has(job, "          persist-credentials: false")) fail("the checkout must not persist credentials (DEC-1005 item 3)");
  const runOf = (step) => step.find((line) => /^ {6}- run: |^ {8}run: /.test(line))?.replace(/^ {6}- run: |^ {8}run: /, "");
  const deploying = steps.filter((step) => runOf(step) === DEPLOY_STEP.run);
  if (deploying.length !== 1 || steps.at(-1) !== deploying[0]) fail(`must end with the one step that runs ${DEPLOY_STEP.run} (DEC-1005 item 1)`);
  if (job.some((line) => /^\s*(?:-\s+)?cache(?:-dependency-path)?:|actions\/cache@/.test(line))) fail("the deploy job may restore or save no cache, which pull request runs could share with the job that holds the token");
  const installs = steps.map(runOf).filter((run) => /^npm (?:ci|install|i)\b/.test(run ?? ""));
  if (installs.length !== 1 || installs[0] !== "npm ci --ignore-scripts") fail("the deploy job must install once, with npm ci --ignore-scripts, so no dependency's install script runs beside the token");
  const buildAt = steps.findIndex((step) => runOf(step) === "npm run cf:build");
  if (buildAt < 0 || buildAt > steps.length - 2) fail("must build the Worker with npm run cf:build before it deploys (DEC-1005 item 1)");
  for (const step of steps) {
    const text = step.join("\n");
    const isDeploy = step === deploying[0];
    const secrets = references(text, "secrets");
    const vars = references(text, "vars");
    if (secrets.some((name) => !isDeploy || !DEPLOY_STEP.secrets.includes(name))) fail(`only the deploy step may read a secret, and only ${DEPLOY_STEP.secrets.join(", ")} (DEC-1005 item 2)`);
    if (vars.some((name) => (isDeploy ? !DEPLOY_STEP.vars.includes(name) : !PUBLIC_VARS.test(name)))) fail(`the build may read only NEXT_PUBLIC_ variables, and the deploy step only ${DEPLOY_STEP.vars.join(", ")} (DEC-1005 item 2)`);
  }
  const outsideSteps = [...[...blocks.entries()].filter(([key]) => key !== "jobs").flatMap(([, block]) => block), ...header, ...settings].join("\n");
  const gated = outsideSteps.replace(DEPLOY_IF, "");
  if (references(gated, "secrets").length > 0 || references(gated, "vars").length > 0) fail("only a step may read a secret or a variable, never the whole job (DEC-1005 item 2)");
  const sanctioned = deploying.length === 1 ? lines.filter((line) => !deploying[0].includes(line)) : lines;
  problems.push(...deployProblems(DEPLOY_WORKFLOW, sanctioned));
  return [...new Set(problems)];
}

function workflowProblems(tree) {
  const workflows = existsSync(WORKFLOWS) ? readdirSync(WORKFLOWS).filter((name) => /\.ya?ml$/.test(name)).map((name) => `../.github/workflows/${name}`) : [];
  const problems = [];
  for (const path of new Set([WEB_WORKFLOW, ...workflows])) {
    if (path === DEPLOY_WORKFLOW || !tree.exists(path)) continue;
    const text = tree.read(path);
    problems.push(...deployProblems(path, text.split("\n")));
    const pin = PINNED_PRODUCTION_WORKFLOWS[path];
    if (pin !== undefined) {
      if (createHash("sha256").update(text).digest("hex") !== pin) problems.push(`${path}: differs from the reviewed text pinned in PINNED_PRODUCTION_WORKFLOWS; re-read it and move the pin in a reviewed change (DEC-1091)`);
      continue;
    }
    if (/^\s*environment:.*\bproduction\b|^\s*name:\s*["']?production\b/im.test(text)) problems.push(`${path}: names the production environment; only the deploy job of ${DEPLOY_WORKFLOW} may (ES-12, DEC-1005 item 2)`);
    if (/CLOUDFLARE_/.test(text)) problems.push(`${path}: holds a CLOUDFLARE_ value; only the deploy step of ${DEPLOY_WORKFLOW} reads the token and the account (DEC-1005 item 2)`);
  }
  problems.push(...deployWorkflowProblems(tree));
  problems.push(...deployProblems("package.json", Object.values(JSON.parse(tree.read("package.json")).scripts ?? {}).map(String)));
  if (!tree.exists(WEB_WORKFLOW)) return [...problems, `${WEB_WORKFLOW}: missing`];
  const yml = tree.read(WEB_WORKFLOW);
  if (!/npm run cf:build/.test(yml)) problems.push(`${WEB_WORKFLOW}: must build the Worker with npm run cf:build`);
  if (!/wrangler deploy --dry-run --outdir/.test(yml)) problems.push(`${WEB_WORKFLOW}: must bundle the Worker with wrangler deploy --dry-run --outdir`);
  if (!/node scripts\/workers\.mjs size/.test(yml)) problems.push(`${WEB_WORKFLOW}: must check the Worker's budget with node scripts/workers.mjs size`);
  if (!/node scripts\/workers\.mjs smoke/.test(yml)) problems.push(`${WEB_WORKFLOW}: must boot the built Worker and ask it for / with node scripts/workers.mjs smoke`);
  if (/\bsecrets\b|\bvars\./.test(yml)) problems.push(`${WEB_WORKFLOW}: reads a secret or a variable; the pull-request build has no credential (ES-12, DEC-1005 item 2)`);
  if (!/^ {2}pull_request:/m.test(yml) || /^ {2}push:/m.test(yml)) problems.push(`${WEB_WORKFLOW}: runs on pull requests and never on a push; the deploy workflow handles main (DEC-1005 item 1)`);
  return problems;
}

const invokedAsScript = process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  const args = process.argv.slice(2);
  const { status, lines } = args[0] === "smoke" ? await smokeCommand(args) : sizeCommand(args);
  for (const line of lines) (status === 0 ? console.log : console.error)(line);
  process.exitCode = status;
}
