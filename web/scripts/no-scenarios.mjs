// Fails the build if the dev-only scenario switcher reached it, or if a production build (flag unset)
// accepts a scenario. Run after `next build`: it scans the output for the switcher, then starts the
// build and asks for the stale scenario by query parameter and by cookie. The e2e build
// (`OWLHEAD_E2E_SCENARIOS=1`, scripts/e2e-build.mjs) must accept both, so the flag is checked both ways.
import { spawn } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { distDir, isE2eBuild } from "./e2e-build.mjs";

/** The switcher's root carries this as its `data-slot`; minifiers keep string literals. */
export const SWITCHER_MARKER = "scenario-switcher";
const COOKIE = "mandate-scenario";
const DEGRADED = 'data-slot="status-strip" data-degraded=""';

export function findSwitcher(nextDir) {
  const scanned = ["static", "server"].flatMap((d) =>
    readdirSync(join(nextDir, d), { withFileTypes: true, recursive: true })
      .filter((e) => e.isFile() && /\.(m?js|html)$/.test(e.name))
      .map((e) => join(e.parentPath, e.name)),
  );
  if (!scanned.some((f) => f.endsWith(".js"))) throw new Error(`no JavaScript under ${nextDir}/static or /server; build first`);
  const hits = scanned.filter((f) => readFileSync(f, "utf8").includes(SWITCHER_MARKER)).map((f) => relative(nextDir, f));
  return { scanned: scanned.length, hits };
}

/** What the running build does with `?scenario=stale` and with the scenario cookie. */
export async function probe(origin) {
  const byParam = await fetch(`${origin}/?scenario=stale`, { redirect: "manual" });
  const setCookie = byParam.headers.getSetCookie().join("; ");
  const byCookie = await fetch(`${origin}/`, { headers: { cookie: `${COOKIE}=stale` } });
  const html = await byCookie.text();
  return {
    paramStatus: byParam.status,
    paramSetsCookie: setCookie.includes(`${COOKIE}=stale`),
    cookieRendersStale: html.includes(DEGRADED),
  };
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer().listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
    server.on("error", reject);
  });
}

async function withBuild(run) {
  const port = await freePort();
  const origin = `http://127.0.0.1:${port}`;
  const next = spawn(join("node_modules", ".bin", "next"), ["start", "--hostname", "127.0.0.1", "--port", String(port)], {
    stdio: ["ignore", "ignore", "inherit"],
  });
  try {
    for (let i = 0; ; i++) {
      if (next.exitCode !== null) throw new Error(`next start exited with ${next.exitCode}`);
      try {
        await fetch(origin, { redirect: "manual" });
        break;
      } catch {
        if (i > 300) throw new Error("next start did not answer within 30 s");
        await new Promise((r) => setTimeout(r, 100));
      }
    }
    return await run(origin);
  } finally {
    next.kill();
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = distDir();
  const e2e = isE2eBuild();
  const failures = [];
  const { scanned, hits } = findSwitcher(join(process.cwd(), dir));
  if (hits.length) failures.push(`the dev-only scenario switcher is in the build:\n    ${hits.join("\n    ")}`);
  const seen = await withBuild(probe);
  const accepts = seen.paramStatus === 307 && seen.paramSetsCookie && seen.cookieRendersStale;
  const refuses = seen.paramStatus === 200 && !seen.paramSetsCookie && !seen.cookieRendersStale;
  if (e2e && !accepts) failures.push(`the e2e build must accept ?scenario=stale and its cookie: ${JSON.stringify(seen)}`);
  if (!e2e && !refuses) failures.push(`a production build must ignore ?scenario= and the scenario cookie: ${JSON.stringify(seen)}`);
  if (failures.length) {
    console.error(`no-scenarios (${dir}):\n  ${failures.join("\n  ")}`);
    process.exit(1);
  }
  console.log(
    `no-scenarios: ${scanned} files in ${dir}/static and ${dir}/server, no scenario switcher; ` +
      (e2e ? "the e2e build accepts ?scenario= and its cookie" : "?scenario= and the scenario cookie are ignored"),
  );
}
