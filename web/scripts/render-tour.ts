/**
 * Renders Tour.mp4, the landing desktop's explainer, from the scenes in src/components/site/tour.ts.
 * Each frame is drawn in headless Chromium by `tour-scenes.js`, in the site's own faces and colours
 * and with the product's own owls (`owl-sprite.ts`, the perch's five agents in `perch.ts`), and
 * piped to ffmpeg. Under it, Scott Joplin's 1916 piano roll of the Maple Leaf Rag (public domain,
 * from Wikimedia Commons) and the synthesized effects of `tour-sound.ts`, levelled to -14 LUFS, the
 * loudness streaming players play at. Writes public/video/owlhead-tour.{mp4,jpg,vtt}.
 *
 *   node --experimental-strip-types scripts/render-tour.ts
 *   node --experimental-strip-types scripts/render-tour.ts --stills 3 12 20   (single frames, to check a scene)
 *   node --experimental-strip-types scripts/render-tour.ts --sound            (only the mix, with its levels)
 */
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { chromium, type Page } from "playwright";
import { type OwlMood, beakFor, bodyRects, eyeRects, owlShape } from "../src/components/domain/owl-sprite.ts";
import { PERCH } from "../src/components/site/perch.ts";
import { TOUR, TOUR_SECONDS, tourVtt } from "../src/components/site/tour.ts";
import { CUES, FPS, RULES_TEXT, WIPE } from "./tour-cues.ts";
import { RATE, tourSfx, wav } from "./tour-sound.ts";

const WEB = join(import.meta.dirname, "..");
const OUT = join(WEB, "public/video");
const TMP = join(tmpdir(), "owlhead-tour");
const MUSIC = "https://upload.wikimedia.org/wikipedia/commons/transcoded/d/db/Maple_leaf_rag_-_played_by_Scott_Joplin_1916_V2.ogg/Maple_leaf_rag_-_played_by_Scott_Joplin_1916_V2.ogg.mp3";
/** Integrated loudness, in LUFS, and the true-peak ceiling, in dBTP. */
const LOUDNESS = -14;
const CEILING = -1.5;
/** The stems before the mix is levelled: the effects sit a little under the rag, so they punctuate it. */
const MUSIC_LUFS = -17;
const SFX_LUFS = -18.5;
/** The landing test's ceiling is 4 MB; this leaves room under it. */
const MAX_BYTES = 3_800_000;

const file = (p: string) => pathToFileURL(join(WEB, p)).href;

/** The light theme's colours, read from the stylesheet so the video matches the site. */
function tokens(): string {
  const css = readFileSync(join(WEB, "src/app/globals.css"), "utf8");
  const root = css.slice(css.indexOf(":root"), css.indexOf("}", css.indexOf(":root")));
  return [...root.matchAll(/--[a-z0-9-]+:\s*oklch\([^)]*\);/g)].map((m) => m[0]).join("\n");
}

const MOODS: OwlMood[] = ["awake", "focused", "asleep", "stopped"];

/** One owl's pixels in every mood, as the `Owl` component draws them. */
function castOwl(seed: string, mood: OwlMood, feathers?: string, beak?: string) {
  const shape = owlShape(seed);
  const plumage = feathers ?? shape.feathers;
  return {
    body: bodyRects(shape, plumage, beak ?? beakFor(plumage)),
    eyes: Object.fromEntries(MOODS.map((m) => [m, eyeRects(m, plumage)])),
    blink: shape.blink,
    mood,
  };
}

/** The brand owl as `BrandOwl` draws it in the light theme's first colour, and the perch's five agents. */
const CAST = {
  brand: castOwl("owlhead", "awake", "var(--series-1)", "var(--highlight)"),
  perch: PERCH.map((p) => castOwl(p.seed, p.mood)),
};

const DATA = { TOUR, CUES, RULES_TEXT, WIPE, FPS, CAST, WAVE: file("public/art/great-wave.jpg"), NIGHT: file("public/art/copenhagen-moonlight.jpg") };

const page = `<!doctype html>
<html><head><meta charset="utf-8"><style>
@font-face { font-family: Pixelify; src: url(${file("node_modules/@fontsource-variable/pixelify-sans/files/pixelify-sans-latin-wght-normal.woff2")}); font-weight: 400 700; }
@font-face { font-family: VT323; src: url(${file("node_modules/@fontsource/vt323/files/vt323-latin-400-normal.woff2")}); }
@font-face { font-family: DotGothic; src: url(${file("node_modules/@fontsource/dotgothic16/files/dotgothic16-latin-400-normal.woff2")}); }
:root { ${tokens()} --owl-eye: var(--card); --owl-pupil: var(--foreground); --owl-line: var(--foreground); }
* { box-sizing: border-box; margin: 0; }
html, body { width: 1280px; height: 720px; overflow: hidden; background: var(--ink); font-family: DotGothic; color: var(--foreground); }
.stage, .cam { position: absolute; inset: 0; }
.cam { will-change: transform; }
.wall { position: absolute; inset: 0 0 44px; background-size: cover; background-position: center; }
.taskbar { position: absolute; inset: auto 0 0; height: 44px; background: var(--muted); border-top: 2px solid var(--card); display: flex; align-items: center; gap: 8px; padding: 0 6px; font-family: Pixelify; font-size: 19px; }
.raised { border: 3px solid; border-color: var(--card) color-mix(in oklab, var(--foreground) 60%, transparent) color-mix(in oklab, var(--foreground) 60%, transparent) var(--card); }
.sunken { border: 3px solid; border-color: color-mix(in oklab, var(--foreground) 60%, transparent) var(--card) var(--card) color-mix(in oklab, var(--foreground) 60%, transparent); }
.start { height: 34px; padding: 0 12px 0 8px; display: flex; align-items: center; gap: 6px; background: var(--muted); }
.clock { margin-left: auto; height: 34px; padding: 0 14px; display: flex; align-items: center; font-family: VT323; font-size: 24px; }
.win { position: absolute; background: var(--muted); padding: 3px; outline: 1px solid color-mix(in oklab, var(--foreground) 70%, transparent); display: flex; flex-direction: column; }
.title { height: 32px; background: var(--foreground); color: var(--card); font-family: Pixelify; font-size: 19px; display: flex; align-items: center; gap: 8px; padding: 0 8px 0 5px; }
.title .x { margin-left: auto; display: flex; gap: 3px; }
.title .x span { width: 22px; height: 20px; background: var(--muted); }
.body { flex: 1; background: var(--card); margin-top: 3px; padding: 14px 18px; overflow: hidden; }
.mono { font-family: VT323; }
.pix { font-family: Pixelify; }
.caption { position: absolute; left: 50%; bottom: 62px; transform: translateX(-50%); width: 1060px; background: color-mix(in oklab, var(--ink) 90%, transparent); color: var(--ink-foreground); font-family: VT323; font-size: 33px; line-height: 1.12; padding: 12px 22px 14px; text-align: center; outline: 2px solid var(--ink-line); }
.caption b { display: block; font-family: Pixelify; font-weight: 600; font-size: 17px; letter-spacing: .12em; color: var(--highlight); text-transform: uppercase; padding-bottom: 4px; }
.owl { display: block; flex-shrink: 0; overflow: visible; }
.btn { font-family: Pixelify; font-size: 19px; padding: 6px 18px; background: var(--muted); display: inline-block; }
.btn.go { background: var(--highlight); outline: 1px solid var(--foreground); }
.row { display: flex; justify-content: space-between; gap: 20px; padding: 7px 4px; border-bottom: 1px dashed color-mix(in oklab, var(--foreground) 30%, transparent); font-size: 22px; }
.row span:first-child { color: var(--muted-foreground); }
.lit { background: var(--highlight) !important; }
.box { padding: 12px 16px; background: var(--card); font-size: 21px; width: 440px; }
.arrow { font-family: VT323; font-size: 30px; padding-left: 210px; line-height: 1; height: 26px; }
.cursor { position: absolute; width: 22px; height: 32px; z-index: 50; transform-origin: top left; }
.rec { font-family: VT323; font-size: 25px; display: grid; grid-template-columns: 100px 120px 1fr 180px; column-gap: 12px; line-height: 1.32; }
.bad { color: var(--crimson); }
.good { color: var(--gain); }
.stop { font-family: Pixelify; font-weight: 700; font-size: 26px; color: var(--crimson-foreground); background: var(--crimson); padding: 10px 26px; letter-spacing: .08em; }
.wipe { position: absolute; inset: 0; z-index: 90; }
.wipe i { position: absolute; width: 80px; height: 80px; }
</style></head><body>
<div id="app" class="stage"></div>
<script>window.TOUR_DATA = ${JSON.stringify(DATA)};</script>
<script src="${file("scripts/tour-scenes.js")}"></script>
</body></html>`;

async function open(): Promise<{ tab: Page; close: () => Promise<void> }> {
  mkdirSync(TMP, { recursive: true });
  const html = join(TMP, "tour.html");
  writeFileSync(html, page);
  const browser = await chromium.launch();
  const tab = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  await tab.goto(pathToFileURL(html).href);
  await tab.evaluate(() => document.fonts.ready);
  return { tab, close: () => browser.close() };
}

const draw = (tab: Page, t: number) => tab.evaluate((at) => (window as unknown as { render: (t: number) => void }).render(at), t);

function ffmpeg(args: string[]): string {
  const run = spawnSync("ffmpeg", ["-hide_banner", "-nostats", ...args], { encoding: "utf8", maxBuffer: 64 << 20 });
  if (run.status !== 0) throw new Error(`ffmpeg ${args.join(" ")}\n${run.stderr}`);
  return run.stderr;
}

/** Integrated loudness and true peak of `input` through `filter`, as ffmpeg's loudnorm measures them. */
function measure(input: string[], filter: string): { lufs: number; peak: number } {
  const log = ffmpeg([...input, "-filter_complex", `${filter},loudnorm=print_format=json`, "-f", "null", "-"]);
  const json = JSON.parse(log.slice(log.lastIndexOf("{"), log.lastIndexOf("}") + 1)) as { input_i: string; input_tp: string };
  return { lufs: Number(json.input_i), peak: Number(json.input_tp) };
}

const gainTo = (from: number, to: number) => (10 ** ((to - from) / 20)).toFixed(4);

/** The music and the effects, each set to its level, mixed, raised to `LOUDNESS` and limited under `CEILING`. */
async function sound(): Promise<string> {
  mkdirSync(TMP, { recursive: true });
  const music = join(TMP, "music.mp3");
  if (!existsSync(music)) {
    const res = await fetch(MUSIC, { headers: { "User-Agent": "OwlheadTour/1.0 (landing page video)" } });
    if (!res.ok) throw new Error(`music: ${res.status}`);
    writeFileSync(music, Buffer.from(await res.arrayBuffer()));
  }
  const sfx = join(TMP, "sfx.wav");
  writeFileSync(sfx, wav(tourSfx(TOUR, TOUR_SECONDS)));

  const inputs = ["-i", music, "-i", sfx];
  const rag = (pad: string) => `[${pad}]atrim=0:${TOUR_SECONDS},asetpts=N/SR/TB,aresample=${RATE},aformat=sample_fmts=fltp:channel_layouts=stereo,afade=t=in:d=1,afade=t=out:st=${TOUR_SECONDS - 3}:d=3`;
  const fx = (pad: string) => `[${pad}]aformat=sample_fmts=fltp:channel_layouts=stereo`;
  const stems = { music: measure(["-i", music], rag("0:a")), sfx: measure(["-i", sfx], fx("0:a")) };
  const mix = `${rag("0:a")},volume=${gainTo(stems.music.lufs, MUSIC_LUFS)}[m];${fx("1:a")},volume=${gainTo(stems.sfx.lufs, SFX_LUFS)}[s];[m][s]amix=inputs=2:normalize=0:duration=first`;
  const limit = (gain: string) => `${mix},volume=${gain},alimiter=limit=${(10 ** ((CEILING - 0.6) / 20)).toFixed(4)}:attack=2:release=80:level=0`;

  let gain = gainTo(measure(inputs, mix).lufs, LOUDNESS);
  const first = measure(inputs, limit(gain));
  gain = (Number(gain) * Number(gainTo(first.lufs, LOUDNESS))).toFixed(4);

  const out = join(TMP, "mix.wav");
  ffmpeg(["-y", ...inputs, "-filter_complex", `${limit(gain)},aresample=${RATE}`, "-c:a", "pcm_f32le", out]);
  const level = measure(["-i", out], fx("0:a"));
  console.log(`music ${stems.music.lufs} LUFS, effects ${stems.sfx.lufs} LUFS; mix ${level.lufs} LUFS, true peak ${level.peak} dBTP`);
  return out;
}

async function stills(times: number[]) {
  const { tab, close } = await open();
  const dir = join(TMP, "stills");
  mkdirSync(dir, { recursive: true });
  for (const t of times) {
    await draw(tab, t);
    writeFileSync(join(dir, `${t.toFixed(2).padStart(5, "0")}.png`), await tab.screenshot());
  }
  await close();
  console.log("stills in", dir);
}

/** Every frame, losslessly, so the final encode can be retried at another quality without drawing again. */
async function frames(): Promise<string> {
  const { tab, close } = await open();
  const master = join(TMP, "frames.mkv");
  const ff = spawn("ffmpeg", ["-y", "-loglevel", "error", "-f", "image2pipe", "-framerate", String(FPS), "-i", "-", "-c:v", "libx264", "-preset", "ultrafast", "-qp", "0", "-pix_fmt", "yuv444p", master], {
    stdio: ["pipe", "inherit", "inherit"],
  });
  const total = TOUR_SECONDS * FPS;
  for (let i = 0; i < total; i++) {
    await draw(tab, i / FPS);
    const shot = await tab.screenshot({ type: "png" });
    if (!ff.stdin.write(shot)) await new Promise((r) => ff.stdin.once("drain", r));
    if (i % (FPS * 5) === 0) console.log(`frame ${i}/${total}`);
  }
  ff.stdin.end();
  await new Promise((r, j) => ff.on("close", (code) => (code === 0 ? r(null) : j(new Error(`ffmpeg ${code}`)))));

  await draw(tab, 2.6);
  writeFileSync(join(OUT, "owlhead-tour.jpg"), await tab.screenshot({ type: "jpeg", quality: 80 }));
  await close();
  return master;
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  const audio = await sound();
  const master = await frames();
  const mp4 = join(OUT, "owlhead-tour.mp4");
  for (const crf of [24, 26, 28, 30, 32, 34]) {
    // prettier-ignore
    ffmpeg(["-y", "-i", master, "-i", audio, "-map", "0:v", "-map", "1:a",
      "-c:v", "libx264", "-preset", "slow", "-tune", "animation", "-crf", String(crf), "-pix_fmt", "yuv420p",
      "-c:a", "aac", "-b:a", "128k", "-ar", String(RATE), "-ac", "2", "-movflags", "+faststart", "-t", String(TOUR_SECONDS), mp4]);
    const bytes = statSync(mp4).size;
    console.log(`crf ${crf}: ${bytes} bytes`);
    if (bytes <= MAX_BYTES) break;
  }
  writeFileSync(join(OUT, "owlhead-tour.vtt"), tourVtt());
  console.log("wrote", mp4);
}

const at = process.argv.indexOf("--stills");
if (at >= 0) await stills(process.argv.slice(at + 1).map(Number));
else if (process.argv.includes("--sound")) await sound();
else await main();
