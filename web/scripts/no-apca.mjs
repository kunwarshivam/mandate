// Fails the build if APCA or colorparsley code reached the production output (web/COLOR.md).
// apca-w3 is licensed for the contrast tests only and colorparsley is AGPL-3.0, so neither ships.
// The markers are object keys and literals that minifiers keep: APCA's SA98G constants and
// colorparsley's named-colour table.
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { distDir } from "./e2e-build.mjs";

export const MARKERS = {
  apca: /\b(blkThrs|blkClmp|loBoWoffset|loWoBoffset|scaleBoW|scaleWoB|mOffsetIn|mOffsetOut)\b|1\.9468554433171/,
  colorparsley: /\bcolorParsley\b|aliceblue\s*:\s*["']f0f8ff/,
};

function files(dir) {
  return readdirSync(dir, { withFileTypes: true, recursive: true })
    .filter((e) => e.isFile() && /\.(m?js|html|json)$/.test(e.name))
    .map((e) => join(e.parentPath, e.name));
}

export function findShipped(nextDir) {
  const scanned = ["static", "server"].flatMap((d) => files(join(nextDir, d)));
  if (!scanned.some((f) => f.endsWith(".js"))) throw new Error(`no JavaScript under ${nextDir}/static or /server; build first`);
  const hits = [];
  for (const f of scanned) {
    const text = readFileSync(f, "utf8");
    for (const [name, re] of Object.entries(MARKERS)) if (re.test(text)) hits.push(`${name}: ${relative(nextDir, f)}`);
  }
  return { scanned: scanned.length, hits };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = distDir();
  const nextDir = join(process.cwd(), dir);
  const { scanned, hits } = findShipped(nextDir);
  if (hits.length) {
    console.error(`APCA or colorparsley code is in the production build (they are dev only):\n  ${hits.join("\n  ")}`);
    process.exit(1);
  }
  console.log(`no-apca: ${scanned} files in ${dir}/static and ${dir}/server, no APCA or colorparsley code`);
}
