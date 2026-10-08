// The Cloudflare Workers host (DEC-823, DEC-731). CI builds the Worker with OpenNext (`npm run cf:build`),
// has Wrangler write the bundle it would upload (`wrangler deploy --dry-run --outdir`, which needs no
// Cloudflare account or token), and runs `node scripts/workers.mjs size <outdir>`, which fails when the
// Worker is over its budget, or when the dry run left no Worker to measure. `hostingProblems` lists what would stop the app from running on Workers,
// or leave a second host configured; the unit tests run it against this tree.

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
  void outdir;
  throw new Error("Unimplemented: E11-9");
}

/**
 * One line for each budget the Worker exceeds; none when it fits.
 * @param {{ rawBytes: number, gzipBytes: number }} size
 * @param {{ rawBytes: number, gzipBytes: number }} [budget]
 * @returns {string[]}
 */
export function sizeProblems(size, budget = WORKER_BUDGET) {
  void size;
  void budget;
  throw new Error("Unimplemented: E11-9");
}

/**
 * `node scripts/workers.mjs size <outdir>`, without exiting: status 0 when the Worker fits its budget,
 * 1 when it does not or there is no Worker to measure, 2 for a bad command line; one line each to print.
 * @param {string[]} args
 * @returns {{ status: number, lines: string[] }}
 */
export function sizeCommand(args) {
  void args;
  throw new Error("Unimplemented: E11-9");
}

/**
 * What stops `web/` from running on Workers through OpenNext, one line each.
 * @param {{ exists(path: string): boolean, read(path: string): string, nextConfig: { images?: { unoptimized?: boolean } } }} tree
 *   paths relative to `web/`
 * @returns {string[]}
 */
export function hostingProblems(tree) {
  void tree;
  throw new Error("Unimplemented: E11-9");
}
