// Pending tests for web/ (DEC-750). A test written before its story is `it.skip("pending <story>: …")`
// and must fail at a stub that throws `new Error("Unimplemented: <story>")`. This script fails when:
//   - any `.skip` is not `it.skip` or `test.skip` titled "pending <story>: …" (describe.skip and
//     `.skip.each` included), or a test is skipped any other way (skipIf, runIf, todo, fails, only,
//     xit, xtest, xdescribe, a computed member), or two pending tests in one file share a title;
//   - a pending test passes when un-skipped, or fails without its stub's own report.
// It un-skips each pending test in a temporary copy of its file, runs the copies with Vitest, and
// deletes them. Run it as `npm run pending`.
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const WEB = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(WEB, "src");
const COPY_SUFFIX = ".pending-unskip.test";
const SKIP = /\.\s*skip\b/g;
const SKIP_ONCE = /^\.\s*skip\b/;
/** Ways to not run a test that this script cannot prove fail at a stub; each is refused outright. */
const UNPROVABLE = [
  { pattern: /\.\s*(skipIf|runIf)\b/g, message: "no conditional skip or run; skip a test only as it.skip(\"pending <story>: …\")" },
  { pattern: /\.\s*(todo|fails|only)\b/g, message: "no .todo, .fails or .only; write the test and skip it as pending" },
  { pattern: /(^|[^\w$.])(xit|xtest|xdescribe)\s*\(/g, message: "no xit, xtest or xdescribe; skip a test only as it.skip(\"pending <story>: …\")" },
  { pattern: /\[\s*["'`](skip|skipIf|runIf|todo|fails|only)["'`]\s*\]/g, message: "no computed skip; skip a test only as it.skip(\"pending <story>: …\")" },
];
const PENDING_SKIP = /\b(it|test)\s*\.\s*skip\s*\(\s*(["'])pending ([A-Z][0-9]+[a-z]?-[0-9]+[a-z]?): ((?:(?!\2)[^\\]|\\.)*)\2/y;

function testFiles() {
  return readdirSync(SRC, { recursive: true })
    .map((name) => join(SRC, String(name)))
    .filter((path) => /\.test\.tsx?$/.test(path) && !path.includes(COPY_SUFFIX));
}

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length;
}

const problems = [];
const pending = [];
const copies = [];

for (const file of testFiles()) {
  const text = readFileSync(file, "utf8");
  const rel = relative(WEB, file);
  let unskipped = text;
  const found = [];
  for (const match of text.matchAll(SKIP)) {
    const start = text.lastIndexOf("\n", match.index) + 1;
    const lead = text.slice(start, match.index).match(/(it|test)\s*$/);
    PENDING_SKIP.lastIndex = lead ? match.index - lead[0].length : match.index;
    const pendingMatch = lead ? PENDING_SKIP.exec(text) : null;
    if (!pendingMatch) {
      problems.push(`${rel}:${lineOf(text, match.index)}: a skip must be it.skip("pending <story>: …") (DEC-750)`);
      continue;
    }
    found.push({ index: match.index, story: pendingMatch[3], title: `pending ${pendingMatch[3]}: ${pendingMatch[4]}`, line: lineOf(text, match.index) });
  }
  for (const { pattern, message } of UNPROVABLE) {
    for (const match of text.matchAll(pattern)) {
      const at = match.index + (match[0].length - match[0].trimStart().length);
      problems.push(`${rel}:${lineOf(text, at)}: ${message} (DEC-750)`);
    }
  }
  const titles = new Set();
  for (const f of found) {
    if (titles.has(f.title)) problems.push(`${rel}:${f.line}: "${f.title}" is a second pending test with the same title`);
    titles.add(f.title);
  }
  if (found.length === 0) continue;
  for (const f of [...found].reverse()) {
    unskipped = unskipped.slice(0, f.index) + unskipped.slice(f.index).replace(SKIP_ONCE, "");
  }
  const copy = file.replace(/\.test\.(tsx?)$/, `${COPY_SUFFIX}.$1`);
  for (const f of found) pending.push({ ...f, rel, copy });
  copies.push({ copy, text: unskipped });
}

if (pending.length > 0) {
  const out = mkdtempSync(join(tmpdir(), "mandate-pending-"));
  const report = join(out, "report.json");
  try {
    for (const { copy, text } of copies) writeFileSync(copy, text);
    spawnSync("npx", ["vitest", "run", "--reporter=json", `--outputFile=${report}`, ...copies.map((c) => relative(WEB, c.copy))], { cwd: WEB, stdio: ["ignore", "ignore", "inherit"] });
    const results = JSON.parse(readFileSync(report, "utf8")).testResults;
    for (const p of pending) {
      const fileResult = results.find((r) => r.name === p.copy);
      const test = fileResult?.assertionResults.find((a) => a.title === p.title);
      if (!test) {
        problems.push(`${p.rel}:${p.line}: "${p.title}" did not run when un-skipped`);
      } else if (test.status === "passed") {
        problems.push(`${p.rel}:${p.line}: "${p.title}" passes when un-skipped; delete its .skip`);
      } else if (!test.failureMessages.some((m) => m.includes(`Unimplemented: ${p.story}`))) {
        problems.push(`${p.rel}:${p.line}: "${p.title}" fails without its stub's own report (Unimplemented: ${p.story})`);
      }
    }
  } finally {
    for (const { copy } of copies) rmSync(copy, { force: true });
    rmSync(out, { recursive: true, force: true });
  }
}

if (problems.length > 0) {
  console.error(problems.join("\n"));
  console.error(`\n${problems.length} pending-test problem(s).`);
  process.exit(1);
}
console.log(`pending: ${pending.length} pending test(s), each fails at its stub when un-skipped.`);
