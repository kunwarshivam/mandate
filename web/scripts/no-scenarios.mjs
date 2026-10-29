// Fails the build if the dev-only scenario switcher reached it, or if a production build (flag unset)
// accepts a scenario. Run after `next build`: it scans the output for the switcher and for any compiled
// route under `/dev/` (DEC-2406: the stub sign-in's token route is `next dev` only, so no build, the
// e2e one included, may hold it). Then it starts the build and asks for the stale scenario by query
// parameter and by cookie, for the mock workspace API at `/v1` (DEC-975), which only the e2e build
// serves, for the Kumo specimen, which only the e2e build renders, and for the stub sign-in's token
// route, which no build serves. The e2e build
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
/** The Kumo specimen's root (`/kumo-specimen`); a production build renders its soft 404 instead, status 200 and no specimen. */
const KUMO_SPECIMEN = 'data-specimen="kumo"';

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

/**
 * Every route under `/dev/` the build compiled, from its app-paths manifest and its server output's file
 * paths. Paths, not text: the client bundle names `/dev/stub-token` as the address it asks.
 */
export function findDevRoutes(nextDir) {
  const manifest = JSON.parse(readFileSync(join(nextDir, "server", "app-paths-manifest.json"), "utf8"));
  const listed = Object.keys(manifest).filter((route) => /^\/dev(\/|$)/.test(route));
  const files = readdirSync(join(nextDir, "server", "app"), { withFileTypes: true, recursive: true })
    .filter((e) => e.isFile())
    .map((e) => relative(join(nextDir, "server", "app"), join(e.parentPath, e.name)))
    .filter((path) => /^dev(\/|$)/.test(path));
  return [...listed, ...files.map((f) => `server/app/${f}`)];
}

/** What the running build does with `?scenario=stale` and with the scenario cookie. */
export async function probe(origin) {
  const byParam = await fetch(`${origin}/?scenario=stale`, { redirect: "manual" });
  const setCookie = byParam.headers.getSetCookie().join("; ");
  const byCookie = await fetch(`${origin}/`, { headers: { cookie: `${COOKIE}=stale` } });
  const html = await byCookie.text();
  // Not followed: with sign-in on, a redirect to /login would answer 200 and pass for the mock.
  const mockApi = await fetch(`${origin}/v1/workspaces/ws_probe/health`, { redirect: "manual" });
  // `next dev` only (DEC-2406): no build, the e2e one included, serves the stub sign-in's token route.
  const stubToken = await fetch(`${origin}/dev/stub-token`, { method: "POST", redirect: "manual", headers: { origin, "x-mandate-request": "1" } });
  // The e2e build alone renders the Kumo specimen; any other build answers its not-found page.
  const specimen = await (await fetch(`${origin}/kumo-specimen`, { redirect: "manual" })).text();
  return {
    rendersKumoSpecimen: specimen.includes(KUMO_SPECIMEN),
    stubTokenStatus: stubToken.status,
    mockApiStatus: mockApi.status,
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
    // The probe asks the build about scenarios, so it reads the fixtures whatever source the shell names (DEC-975).
    env: { ...process.env, OWLHEAD_DATA_SOURCE: "fixtures" },
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
  const devRoutes = findDevRoutes(join(process.cwd(), dir));
  if (devRoutes.length) failures.push(`a dev-only route under /dev/ is compiled into the build:\n    ${devRoutes.join("\n    ")}`);
  const seen = await withBuild(probe);
  const accepts = seen.paramStatus === 307 && seen.paramSetsCookie && seen.cookieRendersStale && seen.mockApiStatus === 200 && seen.stubTokenStatus === 404 && seen.rendersKumoSpecimen;
  const refuses = seen.paramStatus === 200 && !seen.paramSetsCookie && !seen.cookieRendersStale && seen.mockApiStatus === 404 && seen.stubTokenStatus === 404 && !seen.rendersKumoSpecimen;
  if (e2e && !accepts) failures.push(`the e2e build must accept ?scenario=stale and its cookie: ${JSON.stringify(seen)}`);
  if (!e2e && !refuses) failures.push(`a production build must ignore ?scenario= and the scenario cookie, and serve no mock API: ${JSON.stringify(seen)}`);
  if (failures.length) {
    console.error(`no-scenarios (${dir}):\n  ${failures.join("\n  ")}`);
    process.exit(1);
  }
  console.log(
    `no-scenarios: ${scanned} files in ${dir}/static and ${dir}/server, no scenario switcher; ` +
      (e2e ? "the e2e build accepts ?scenario= and its cookie" : "?scenario= and the scenario cookie are ignored"),
  );
}
