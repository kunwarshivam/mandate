/**
 * Renders Tour.mp4, the landing desktop's explainer, from the scenes in src/components/site/tour.ts.
 * Each frame is drawn in headless Chromium, in the site's own faces and colours, and piped to ffmpeg,
 * with Scott Joplin's 1916 piano roll of the Maple Leaf Rag (public domain, from Wikimedia Commons)
 * underneath. Writes public/video/owlhead-tour.{mp4,jpg,vtt}.
 *
 *   node --experimental-strip-types scripts/render-tour.ts
 *   node --experimental-strip-types scripts/render-tour.ts --stills 3 12 20   (single frames, to check a scene)
 */
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";
import { TOUR, TOUR_SECONDS, tourVtt } from "../src/components/site/tour.ts";

const WEB = join(import.meta.dirname, "..");
const OUT = join(WEB, "public/video");
const FPS = 24;
const MUSIC = "https://upload.wikimedia.org/wikipedia/commons/transcoded/d/db/Maple_leaf_rag_-_played_by_Scott_Joplin_1916_V2.ogg/Maple_leaf_rag_-_played_by_Scott_Joplin_1916_V2.ogg.mp3";

const file = (p: string) => pathToFileURL(join(WEB, p)).href;

/** The light theme's colours, read from the stylesheet so the video matches the site. */
function tokens(): string {
  const css = readFileSync(join(WEB, "src/app/globals.css"), "utf8");
  const root = css.slice(css.indexOf(":root"), css.indexOf("}", css.indexOf(":root")));
  return [...root.matchAll(/--[a-z0-9-]+:\s*oklch\([^)]*\);/g)].map((m) => m[0]).join("\n");
}

const OWL = ["..kk....kk..", "..kbk..kbk..", "..kbbbbbbk..", ".kwwwbbwwwk.", ".kwkwbbwkwk.", ".kwwwyywwwk.", ".kbbbyybbbk.", "kbbtbbbbtbbk", "kbtbtbbtbtbk", "kbbtbbbbtbbk", ".kbbbbbbbbk.", "..kykkkkyk.."];

const page = `<!doctype html>
<html><head><meta charset="utf-8"><style>
@font-face { font-family: Pixelify; src: url(${file("node_modules/@fontsource-variable/pixelify-sans/files/pixelify-sans-latin-wght-normal.woff2")}); font-weight: 400 700; }
@font-face { font-family: VT323; src: url(${file("node_modules/@fontsource/vt323/files/vt323-latin-400-normal.woff2")}); }
@font-face { font-family: DotGothic; src: url(${file("node_modules/@fontsource/dotgothic16/files/dotgothic16-latin-400-normal.woff2")}); }
:root { ${tokens()} }
* { box-sizing: border-box; margin: 0; }
html, body { width: 1280px; height: 720px; overflow: hidden; background: var(--ink); font-family: DotGothic; color: var(--foreground); }
.stage { position: absolute; inset: 0; }
.wall { position: absolute; inset: 0 0 44px; background-size: cover; background-position: center; }
.taskbar { position: absolute; inset: auto 0 0; height: 44px; background: var(--muted); border-top: 2px solid var(--card); display: flex; align-items: center; gap: 8px; padding: 0 6px; font-family: Pixelify; font-size: 19px; }
.raised { border: 3px solid; border-color: var(--card) color-mix(in oklab, var(--foreground) 60%, transparent) color-mix(in oklab, var(--foreground) 60%, transparent) var(--card); }
.sunken { border: 3px solid; border-color: color-mix(in oklab, var(--foreground) 60%, transparent) var(--card) var(--card) color-mix(in oklab, var(--foreground) 60%, transparent); }
.start { height: 34px; padding: 0 12px; display: flex; align-items: center; gap: 8px; background: var(--muted); }
.clock { margin-left: auto; height: 34px; padding: 0 14px; display: flex; align-items: center; font-family: VT323; font-size: 24px; }
.win { position: absolute; background: var(--muted); padding: 3px; outline: 1px solid color-mix(in oklab, var(--foreground) 70%, transparent); display: flex; flex-direction: column; }
.title { height: 32px; background: var(--foreground); color: var(--card); font-family: Pixelify; font-size: 19px; display: flex; align-items: center; gap: 8px; padding: 0 8px; }
.title .x { margin-left: auto; display: flex; gap: 3px; }
.title .x span { width: 22px; height: 20px; background: var(--muted); }
.body { flex: 1; background: var(--card); margin-top: 3px; padding: 14px 18px; overflow: hidden; }
.mono { font-family: VT323; }
.pix { font-family: Pixelify; }
.caption { position: absolute; left: 50%; bottom: 62px; transform: translateX(-50%); width: 1060px; background: color-mix(in oklab, var(--ink) 90%, transparent); color: var(--ink-foreground); font-family: VT323; font-size: 33px; line-height: 1.12; padding: 12px 22px 14px; text-align: center; outline: 2px solid var(--ink-line); }
.caption b { display: block; font-family: Pixelify; font-weight: 600; font-size: 17px; letter-spacing: .12em; color: var(--highlight); text-transform: uppercase; padding-bottom: 4px; }
.owl { display: grid; }
.btn { font-family: Pixelify; font-size: 19px; padding: 6px 18px; background: var(--muted); display: inline-block; }
.btn.go { background: var(--highlight); outline: 1px solid var(--foreground); }
.row { display: flex; justify-content: space-between; gap: 20px; padding: 7px 0; border-bottom: 1px dashed color-mix(in oklab, var(--foreground) 30%, transparent); font-size: 22px; }
.row span:first-child { color: var(--muted-foreground); }
.lit { background: var(--highlight) !important; }
.box { padding: 12px 16px; background: var(--card); font-size: 21px; width: 440px; }
.arrow { font-family: VT323; font-size: 30px; padding-left: 210px; line-height: 1; height: 26px; }
.cursor { position: absolute; width: 22px; height: 32px; z-index: 50; }
.rec { font-family: VT323; font-size: 25px; display: grid; grid-template-columns: 100px 120px 1fr 180px; column-gap: 12px; line-height: 1.32; }
.bad { color: var(--crimson); }
.good { color: var(--gain); }
.stop { font-family: Pixelify; font-weight: 700; font-size: 26px; color: var(--crimson-foreground); background: var(--crimson); padding: 10px 26px; letter-spacing: .08em; }
</style></head><body>
<div id="app" class="stage"></div>
<script>
const TOUR = ${JSON.stringify(TOUR)};
const OWL = ${JSON.stringify(OWL)};
const WAVE = ${JSON.stringify(file("public/art/great-wave.jpg"))};
const NIGHT = ${JSON.stringify(file("public/art/copenhagen-moonlight.jpg"))};
const INK = { k: "var(--foreground)", b: "var(--series-2)", w: "var(--card)", y: "var(--highlight)", t: "var(--series-5)" };
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const ease = (x) => 1 - Math.pow(1 - clamp(x, 0, 1), 3);
const shut = (c, y) => (y === 3 || y === 5 ? (c === "w" ? "b" : c) : y === 4 && (c === "w" || c === "k") ? "k" : c);
const owl = (px, eyes = true) => '<div class="owl" style="grid-template-columns:repeat(12,' + px + 'px)">' + OWL.map((r, y) => [...r].map((c) => (eyes ? c : shut(c, y))).map((c) => '<i style="width:' + px + 'px;height:' + px + 'px;background:' + (c === "." ? "transparent" : INK[c]) + '"></i>').join("")).join("") + "</div>";
const cursor = (x, y, down) => '<svg class="cursor" style="left:' + x + 'px;top:' + y + 'px" viewBox="0 0 11 16" shape-rendering="crispEdges"><path d="M0 0v14l3-3 2 5 2-1-2-5h4z" fill="' + (down ? "var(--highlight)" : "var(--card)") + '" stroke="var(--foreground)" stroke-width="1"/></svg>';
const win = (x, y, w, h, title, body, extra = "") => '<div class="win raised" style="left:' + x + "px;top:" + y + "px;width:" + w + "px;height:" + h + "px;" + extra + '"><div class="title">' + title + '<span class="x"><span class="raised"></span><span class="raised"></span><span class="raised"></span></span></div><div class="body sunken">' + body + "</div></div>";
const desk = (img, inner, clock) => '<div class="wall" style="background-image:url(' + img + ')"></div>' + inner + '<div class="taskbar"><span class="start raised">' + owl(2) + ' Start</span><span class="clock sunken">' + clock + "</span></div>";
const type = (text, n) => text.slice(0, Math.max(0, Math.floor(n)));
const blink = (t) => (Math.floor(t * 2) % 2 ? "&nbsp;" : "▌");
const at = (t, a, b) => (t < a ? a : t > b ? b : t);
const lerp = (a, b, k) => a + (b - a) * ease(k);

const SCENES = {
  boot(t) {
    const bar = Math.floor(clamp(t / 4, 0, 1) * 22);
    return '<div class="stage" style="display:grid;place-content:center;justify-items:center;gap:22px;background:var(--ink);color:var(--ink-foreground)">' + owl(13) +
      '<div class="pix" style="font-size:72px;font-weight:600;line-height:1">Owlhead <span style="color:var(--highlight)">98</span></div>' +
      '<div class="sunken" style="width:420px;height:30px;padding:3px;display:flex;gap:3px;background:var(--ink)">' + Array.from({ length: bar }, () => '<i style="width:16px;background:var(--highlight)"></i>').join("") + "</div>" +
      '<div class="mono" style="font-size:28px;color:var(--series-5)">Starting Owlhead' + ".".repeat(1 + (Math.floor(t * 3) % 3)) + "</div></div>";
  },
  rules(t) {
    const text = "US stocks only.\\nNo more than $2,000 on any one order.\\nAsk me before buying anything you haven't held.";
    const typed = type(text, t * 30).replace(/\\n/g, "<br>");
    const note = win(90, 60, 600, 300, "mandate.txt - Notepad", '<div class="mono" style="font-size:31px;line-height:1.3">' + typed + blink(t) + "</div>");
    let dialog = "";
    if (t > 4.2) {
      const k = ease((t - 4.2) / 0.4);
      const pressed = t > 7.9;
      dialog = win(700, 150, 500, 330, "Owlhead understood", '<div class="pix" style="font-size:20px;padding-bottom:6px">Here are your rules, in dollars:</div>' +
        '<div class="row"><span>Can buy</span><span>US stocks</span></div><div class="row"><span>Largest order</span><span>$2,000</span></div><div class="row"><span>Asks you before</span><span>anything new to you</span></div>' +
        '<div style="display:flex;gap:10px;justify-content:flex-end;padding-top:16px"><span class="btn ' + (pressed ? "sunken lit" : "raised go") + '">' + (pressed ? "Confirmed" : "Confirm") + '</span><span class="btn raised">Edit</span></div>', "transform:scale(" + (0.92 + 0.08 * k) + ");opacity:" + k);
      const cx = lerp(1180, 1010, (t - 5.8) / 1.8), cy = lerp(640, 440, (t - 5.8) / 1.8);
      dialog += cursor(cx, cy, t > 7.8 && t < 8.2);
    }
    return desk(WAVE, note + dialog, "9:29 AM");
  },
  ideas(t) {
    const lines = [
      ["Idea 14", "Buy XYZ"],
      ["Why", "Orders have grown three quarters running."],
      ["Wrong if", "Next quarter's orders fall."],
      ["Exit plan", "Stop $84 · target $104 · 30 days"],
    ];
    const shown = lines.filter((_, i) => t > 1.2 + i * 1.3).map(([a, b]) => '<div class="row"><span>' + a + "</span><span>" + b + "</span></div>").join("");
    const reading = '<div class="mono" style="font-size:25px;color:var(--muted-foreground);padding-bottom:8px">Reading filings, news and prices' + ".".repeat(1 + (Math.floor(t * 3) % 3)) + "</div>";
    const body = '<div style="display:flex;gap:26px"><div style="padding-top:8px">' + owl(9, Math.floor(t * 1.4) % 5 !== 0) + '</div><div style="flex:1">' + reading + shown + "</div></div>";
    return desk(WAVE, win(170, 70, 940, 310, "ideas.txt - Owlhead", body), "9:31 AM");
  },
  orders(t) {
    const steps = ["Idea 14: buy XYZ", "Sized: 20 XYZ for $1,840", "Checked: under $2,000, a US stock", "New to you, so it asks first"];
    const lit = Math.floor(clamp((t - 0.4) / 1.3, -1, 3));
    const flow = '<div style="position:absolute;left:80px;top:56px;display:grid">' + steps.map((s, i) => (i ? '<div class="arrow">↓</div>' : "") + '<div class="box ' + (i <= lit ? "raised lit" : "raised") + '"><span class="pix">' + s + (i <= lit && i > 0 && i < 3 ? ' <span class="good">✓</span>' : "") + "</span></div>").join("") + "</div>";
    const approved = t > 7.3;
    const card = '<div style="background:var(--card);padding:14px;font-size:19px;line-height:1.3">' + (approved
      ? '<div class="pix good" style="font-size:24px;font-weight:600">Approved.</div><div>Order sent to your broker. It\\'s in the record.</div>'
      : '<div class="pix" style="font-weight:600;font-size:21px;padding-bottom:6px">Owlhead wants to buy</div><div style="font-size:24px">20 XYZ · $1,840</div><div style="color:var(--muted-foreground);padding:6px 0 12px">Your rules say to ask before buying anything you haven\\'t held.</div><div style="display:flex;gap:8px"><span class="btn ' + (t > 6.9 ? "sunken lit" : "raised go") + '">Approve</span><span class="btn raised">Decline</span></div>') + "</div>";
    const phoneIn = ease((t - 4.4) / 0.5);
    const phone = t > 4.4 ? '<div style="position:absolute;left:760px;top:' + (40 + (1 - phoneIn) * 80) + 'px;width:330px;height:560px;background:var(--ink);border-radius:34px;padding:52px 18px;opacity:' + phoneIn + '"><div class="mono" style="color:var(--ink-foreground);font-size:22px;text-align:center;padding-bottom:12px">9:32</div>' + card + "</div>" : "";
    const tap = t > 5.2 ? cursor(lerp(1150, 830, (t - 5.2) / 1.5), lerp(640, 432, (t - 5.2) / 1.5), t > 6.9 && t < 7.3) : "";
    return desk(WAVE, flow + phone + tap, "9:32 AM");
  },
  record(t) {
    const rows = [
      ["09:30:58", "read", "XYZ quarterly report", "a41f9b"],
      ["09:31:02", "idea", "Idea 14: buy XYZ", "7c02e1"],
      ["09:31:03", "sized", "20 XYZ, $1,840", "3f9a44"],
      ["09:31:03", "checked", "within your rules", "e5b710"],
      ["09:31:04", "asked", "the owner, new to you", "9d2c8f"],
      ["09:32:10", "approved", "by the owner, on phone", "b06e53"],
      ["09:32:11", "sent", "to Alpaca, on paper", "4c8ad2"],
    ];
    const edited = t > 6;
    const body = '<div class="rec"><span class="pix" style="font-size:17px;color:var(--muted-foreground)">TIME</span><span class="pix" style="font-size:17px;color:var(--muted-foreground)">WHAT</span><span class="pix" style="font-size:17px;color:var(--muted-foreground)">DETAIL</span><span class="pix" style="font-size:17px;color:var(--muted-foreground)">HASH</span>' +
      rows.filter((_, i) => t > 0.4 + i * 0.6).map((r, i) => {
        const broken = edited && i >= 2;
        const detail = i === 2 && edited ? '<span class="bad" style="background:color-mix(in oklab,var(--crimson) 14%,transparent)">200 XYZ, $18,400</span>' : r[2];
        return "<span>" + r[0] + "</span><span>" + r[1] + "</span><span>" + detail + '</span><span class="' + (broken ? "bad" : "good") + '">' + (broken ? "✗ no match" : "✓ " + r[3]) + "</span>";
      }).join("") + "</div>";
    return desk(WAVE, win(110, 50, 1060, 370, "The record - Owlhead", body), "9:33 AM");
  },
  check(t) {
    const items = ["Paper first, with simulated money.", "One Stop button halts every agent.", "It can't take money out of your account."];
    const pressed = t > 4.2 && t < 4.6;
    const halted = t > 4.4;
    const list = items.filter((_, i) => t > 0.4 + i * 2.3).map((s) => '<div style="display:flex;gap:14px;align-items:center;font-size:27px;padding:10px 0"><span class="good pix" style="font-size:30px">✓</span>' + s + "</div>").join("");
    const stop = t > 2.7 ? '<div style="display:flex;align-items:center;gap:22px;padding-top:12px"><span class="stop ' + (pressed ? "sunken" : "raised") + '">STOP</span><span class="mono" style="font-size:26px;color:var(--muted-foreground)">' + (halted ? "3 agents halted · 2 open orders cancelled" : "3 agents trading") + "</span></div>" : "";
    const owls = '<div style="display:flex;gap:18px;position:absolute;right:40px;top:30px">' + [0, 1, 2].map(() => owl(6, !halted)).join("") + "</div>";
    const cur = t > 2.9 ? cursor(lerp(1100, 290, (t - 2.9) / 1.2), lerp(600, 380, (t - 2.9) / 1.2), pressed) : "";
    return desk(WAVE, win(150, 70, 980, 330, "How it stays in check", '<div style="position:relative">' + owls + list + stop + "</div>") + cur, "9:34 AM");
  },
  end(t) {
    const fade = clamp((t - 6) / 1, 0, 1);
    const body = '<div style="display:grid;justify-items:center;gap:16px;padding-top:16px;text-align:center">' + owl(10) + '<div class="pix" style="font-size:60px;font-weight:600;line-height:1">Owlhead</div><div style="font-size:25px;max-width:560px">A trading agent for your own brokerage account. It works inside rules you write, and it writes down every decision it makes.</div><div class="btn raised go" style="font-size:22px">Ask for a place at owlhead.ai</div></div>';
    return desk(NIGHT, win(290, 36, 700, 470, "Owlhead", body), "9:35 AM") + '<div class="stage" style="background:var(--ink);opacity:' + fade + '"></div>';
  },
};

window.render = (t) => {
  const s = TOUR.find((x) => t >= x.start && t < x.end) ?? TOUR[TOUR.length - 1];
  const local = t - s.start;
  const n = TOUR.indexOf(s);
  const label = s.id === "boot" || s.id === "end" ? "Owlhead" : "Step " + n + " of 5";
  const cap = local < 0.25 || (s.id === "end" && local > 6) ? "" : '<div class="caption"><b>' + label + "</b>" + s.caption + "</div>";
  document.getElementById("app").innerHTML = SCENES[s.id](local) + cap;
};
</script></body></html>`;

async function stills(times: number[]) {
  const html = join(tmpdir(), "owlhead-tour.html");
  writeFileSync(html, page);
  const browser = await chromium.launch();
  const tab = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  await tab.goto(pathToFileURL(html).href);
  await tab.evaluate(() => document.fonts.ready);
  const dir = join(tmpdir(), "owlhead-tour-stills");
  mkdirSync(dir, { recursive: true });
  for (const t of times) {
    await tab.evaluate((at) => (window as unknown as { render: (t: number) => void }).render(at), t);
    writeFileSync(join(dir, `${String(t).padStart(5, "0")}.png`), await tab.screenshot());
  }
  await browser.close();
  console.log("stills in", dir);
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  const html = join(tmpdir(), "owlhead-tour.html");
  writeFileSync(html, page);
  const music = join(tmpdir(), "owlhead-tour-music.mp3");
  const res = await fetch(MUSIC, { headers: { "User-Agent": "OwlheadTour/1.0 (landing page video)" } });
  if (!res.ok) throw new Error(`music: ${res.status}`);
  writeFileSync(music, Buffer.from(await res.arrayBuffer()));

  const browser = await chromium.launch();
  const tab = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  await tab.goto(pathToFileURL(html).href);
  await tab.evaluate(() => document.fonts.ready);

  const mp4 = join(OUT, "owlhead-tour.mp4");
  // prettier-ignore
  const ff = spawn("ffmpeg", ["-y", "-loglevel", "error", "-f", "image2pipe", "-framerate", String(FPS), "-i", "-", "-i", music,
    "-filter_complex", `[1:a]atrim=0:${TOUR_SECONDS},afade=t=in:d=1,afade=t=out:st=${TOUR_SECONDS - 3}:d=3,volume=0.55[a]`, "-map", "0:v", "-map", "[a]",
    "-c:v", "libx264", "-preset", "slow", "-tune", "animation", "-crf", "27", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "64k", "-ac", "1",
    "-movflags", "+faststart", "-t", String(TOUR_SECONDS), mp4], { stdio: ["pipe", "inherit", "inherit"] });

  const frames = TOUR_SECONDS * FPS;
  for (let i = 0; i < frames; i++) {
    await tab.evaluate((t) => (window as unknown as { render: (t: number) => void }).render(t), i / FPS);
    const shot = await tab.screenshot({ type: "jpeg", quality: 92 });
    if (!ff.stdin.write(shot)) await new Promise((r) => ff.stdin.once("drain", r));
    if (i % (FPS * 5) === 0) console.log(`frame ${i}/${frames}`);
  }
  ff.stdin.end();
  await new Promise((r, j) => ff.on("close", (code) => (code === 0 ? r(null) : j(new Error(`ffmpeg ${code}`)))));

  await tab.evaluate(() => (window as unknown as { render: (t: number) => void }).render(2.6));
  writeFileSync(join(OUT, "owlhead-tour.jpg"), await tab.screenshot({ type: "jpeg", quality: 80 }));
  writeFileSync(join(OUT, "owlhead-tour.vtt"), tourVtt());
  await browser.close();
  console.log("wrote", mp4);
}

const at = process.argv.indexOf("--stills");
await (at >= 0 ? stills(process.argv.slice(at + 1).map(Number)) : main());
