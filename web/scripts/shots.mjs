// Every listed screen on the fixture scenarios, at phone and desktop widths in both themes, as
// PNG files under .shots/, with a contact sheet (index.html) that puts the golden path first
// (DEC-516 item 3). Review from pictures: a PR that changes a screen attaches these, before and
// after; the weekly critique reads the same sheet.
//
// Usage: a server on http://127.0.0.1:4317 (`npm run dev`, which answers `?scenario=`; a
// production `npm start` ignores it and shows `normal` only), then `npm run shots`.
// Options: --out <dir> (default .shots), --only golden (the golden path alone), --width 390|1440.
// SHOTS_CHROMIUM=<path> runs a Chromium of your own where Playwright has not downloaded one.

import { mkdir, writeFile, rm } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright";

const BASE = process.env.SHOTS_BASE ?? "http://127.0.0.1:4317";
const args = process.argv.slice(2);
const arg = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const OUT = path.resolve(arg("out", ".shots"));
const ONLY = arg("only", "all");
const WIDTHS = arg("width", null) ? [Number(arg("width"))] : [1440, 390];

const AGENT = "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC";
const APPROVAL = "apr_01JBM9S346Q3D25VT4F5V37E3S";

/** The golden path, in the order an owner meets it: the ten minutes that must be perfect. */
const GOLDEN = [
  ["welcome", "/welcome", "normal"],
  ["login", "/login", "normal"],
  ["new-agent", "/agents/new", "normal"],
  ["home-request", "/?scenario=approvals", "approvals"],
  ["request", `/approvals/${APPROVAL}?scenario=approvals`, "approvals"],
  ["agent", `/agents/${AGENT}`, "normal"],
  ["agent-decisions", `/agents/${AGENT}/decisions`, "normal"],
  ["agent-mandate", `/agents/${AGENT}/mandate`, "normal"],
  ["stop-sheet", "/", "normal", "stop"],
];

/** Every listed screen on the normal scenario. */
const SCREENS = [
  ["home", "/"],
  ["agents", "/agents"],
  ["approvals", "/approvals"],
  ["alerts", "/alerts"],
  ["positions", "/positions"],
  ["messages", "/messages"],
  ["messages-agent", `/messages/${AGENT}`],
  ["audit", "/audit"],
  ["audit-decisions", "/audit/decisions"],
  ["audit-timeline", "/audit/timeline"],
  ["settings", "/settings"],
  ["palette", "/", "normal", "palette"],
];

/** Home and the agent under each stress scenario, desktop and light only. */
const SCENARIOS = ["stale", "drawdown", "paused", "unreachable", "empty", "reconciliation", "unknown-order"];

async function shoot(page, { path: url, overlay }, file) {
  await page.goto(`${BASE}${url}`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(900);
  if (overlay === "stop") {
    await page.getByRole("button", { name: /^Stop$/ }).first().click();
    await page.waitForTimeout(500);
  }
  if (overlay === "palette") {
    await page.keyboard.press("Meta+k");
    await page.waitForTimeout(500);
  }
  await page.screenshot({ path: file, fullPage: false });
}

async function main() {
  const probe = await fetch(BASE).catch(() => null);
  if (!probe || !probe.ok) {
    console.error(`No server answers at ${BASE}. Start one with \`npm run dev\` first.`);
    process.exit(2);
  }
  await rm(OUT, { recursive: true, force: true });
  await mkdir(OUT, { recursive: true });
  const browser = await chromium.launch(process.env.SHOTS_CHROMIUM ? { executablePath: process.env.SHOTS_CHROMIUM } : {});
  const entries = [];
  const plan = [];
  for (const [name, url, scenario, overlay] of GOLDEN) for (const width of WIDTHS) for (const theme of ["light", "dark"]) plan.push({ group: "Golden path", name, path: url, scenario, overlay, width, theme });
  if (ONLY !== "golden") {
    for (const [name, url, scenario = "normal", overlay] of SCREENS) for (const width of WIDTHS) for (const theme of ["light", "dark"]) plan.push({ group: "Every screen", name, path: url, scenario, overlay, width, theme });
    for (const scenario of SCENARIOS) {
      plan.push({ group: "Scenarios", name: `home-${scenario}`, path: `/?scenario=${scenario}`, scenario, width: 1440, theme: "light" });
      plan.push({ group: "Scenarios", name: `agent-${scenario}`, path: `/agents/${AGENT}?scenario=${scenario}`, scenario, width: 1440, theme: "light" });
    }
  }
  for (const item of plan) {
    const context = await browser.newContext({ viewport: { width: item.width, height: item.width < 600 ? 844 : 900 }, colorScheme: item.theme, deviceScaleFactor: 1 });
    const page = await context.newPage();
    const file = `${item.name}-${item.width}-${item.theme}.png`;
    try {
      await shoot(page, item, path.join(OUT, file));
      entries.push({ ...item, file });
      process.stdout.write(`${file}\n`);
    } catch (error) {
      process.stdout.write(`${file}: ${String(error).split("\n")[0]}\n`);
    }
    await context.close();
  }
  await browser.close();
  await writeFile(path.join(OUT, "index.html"), sheet(entries));
  console.log(`${entries.length} screens in ${OUT}; open ${path.join(OUT, "index.html")}`);
}

function sheet(entries) {
  const groups = [...new Set(entries.map((e) => e.group))];
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const section = (group) =>
    `<h2>${esc(group)}</h2><div class="grid">${entries
      .filter((e) => e.group === group)
      .map((e) => `<figure><img src="${esc(e.file)}" loading="lazy" alt="${esc(e.name)} at ${e.width}px, ${e.theme}"><figcaption>${esc(e.name)} · ${e.width}px · ${e.theme}${e.scenario !== "normal" ? ` · ${esc(e.scenario)}` : ""}</figcaption></figure>`)
      .join("")}</div>`;
  return `<!doctype html><meta charset="utf-8"><title>Owlhead screens</title>
<style>body{font:14px/1.4 system-ui;margin:24px;background:#f5f7f9;color:#14161a}h2{margin:32px 0 12px}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(360px,1fr));gap:16px}figure{margin:0;background:#fff;border:1px solid #d9dde3;border-radius:8px;overflow:hidden}
img{display:block;width:100%;height:auto}figcaption{padding:6px 10px;color:#5b6270;border-top:1px solid #e6e9ee}</style>
<h1>Owlhead screens, ${new Date().toISOString().slice(0, 16).replace("T", " ")} UTC</h1>
<p>${entries.length} captures. The golden path first: the ten minutes that must be perfect on every release.</p>
${groups.map(section).join("")}`;
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
