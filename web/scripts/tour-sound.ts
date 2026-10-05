/**
 * The tour's sound effects, synthesized: every click, keystroke, pop and chime lands on the cue the
 * scenes draw from (`tour-cues.ts`), panned to where it happens on screen. A seeded generator makes
 * the noise, so the track is the same on every render. `render-tour.ts` mixes it under the music.
 */
import type { Scene } from "../src/components/site/tour.ts";
import { CUES, RULES_TEXT, WIPE } from "./tour-cues.ts";

export const RATE = 48_000;

type Wave = "sine" | "square" | "triangle" | "saw";

interface Voice {
  dur: number;
  gain: number;
  /** -1 left to 1 right. */
  pan?: number;
  attack?: number;
  /** How fast it dies away: higher is shorter. */
  decay?: number;
  /** A one-pole low-pass, in Hz, to soften a square's edge or darken a noise. */
  lowpass?: number | [number, number];
}

interface Tone extends Voice {
  freq: number;
  /** The pitch it glides to by the end, exponentially. */
  to?: number;
  wave?: Wave;
}

/** mulberry32. */
function generator(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const midi = (n: number) => 440 * 2 ** ((n - 69) / 12);

function oscillate(wave: Wave, cycles: number): number {
  const p = cycles - Math.floor(cycles);
  switch (wave) {
    case "sine":
      return Math.sin(2 * Math.PI * p);
    case "square":
      return p < 0.5 ? 1 : -1;
    case "triangle":
      return 1 - 4 * Math.abs(p - 0.5);
    case "saw":
      return 2 * p - 1;
    default: {
      const unhandled: never = wave;
      throw new Error(`unhandled wave ${String(unhandled)}`);
    }
  }
}

class Track {
  readonly left: Float32Array;
  readonly right: Float32Array;
  private readonly random = generator(0x0071ead);

  constructor(seconds: number) {
    this.left = new Float32Array(Math.ceil(seconds * RATE));
    this.right = new Float32Array(this.left.length);
  }

  rand(): number {
    return this.random();
  }

  private voice(at: number, v: Voice, source: (t: number) => number) {
    const start = Math.round(at * RATE);
    const n = Math.round(v.dur * RATE);
    const angle = ((clamp(v.pan ?? 0, -1, 1) + 1) * Math.PI) / 4;
    const [l, r] = [Math.cos(angle) * v.gain, Math.sin(angle) * v.gain];
    const attack = v.attack ?? 0.003;
    const decay = v.decay ?? 6;
    const [lpFrom, lpTo] = Array.isArray(v.lowpass) ? v.lowpass : [v.lowpass ?? 0, v.lowpass ?? 0];
    let y = 0;
    for (let i = 0; i < n && start + i < this.left.length; i++) {
      if (start + i < 0) continue;
      const t = i / RATE;
      const k = i / n;
      const env = Math.min(1, t / attack) * Math.exp(-decay * k) * (1 - k);
      let x = source(t);
      if (lpFrom > 0) {
        const fc = lpFrom * (lpTo / lpFrom) ** k;
        y += (1 - Math.exp((-2 * Math.PI * fc) / RATE)) * (x - y);
        x = y;
      }
      this.left[start + i] += x * env * l;
      this.right[start + i] += x * env * r;
    }
  }

  tone(at: number, v: Tone) {
    const wave = v.wave ?? "sine";
    const ratio = (v.to ?? v.freq) / v.freq;
    let cycles = 0;
    let last = 0;
    this.voice(at, v, (t) => {
      cycles += v.freq * ratio ** (t / v.dur) * (t - last);
      last = t;
      return oscillate(wave, cycles);
    });
  }

  noise(at: number, v: Voice) {
    this.voice(at, v, () => this.random() * 2 - 1);
  }
}

const clamp = (v: number, a: number, b: number) => Math.min(b, Math.max(a, v));

/** Where an x on the 1280-pixel stage sits between the speakers. */
const panAt = (x: number) => clamp(((x - 640) / 640) * 0.7, -0.7, 0.7);

const PENTATONIC = [0, 2, 4, 7, 9, 12, 14, 16, 19, 21];

/** The tour's cues as sounds. */
class Foley {
  readonly track: Track;

  constructor(track: Track) {
    this.track = track;
  }

  key(at: number, pan: number) {
    const r = this.track.rand();
    this.track.noise(at, { dur: 0.028, gain: 0.2, pan, decay: 9, lowpass: 5200 + r * 1800 });
    this.track.tone(at, { freq: 1700 + r * 500, dur: 0.018, gain: 0.05, wave: "square", pan, lowpass: 4500 });
  }

  carriage(at: number, pan: number) {
    this.track.noise(at, { dur: 0.08, gain: 0.32, pan, decay: 5, lowpass: 1600 });
    this.track.tone(at, { freq: 210, to: 120, dur: 0.07, gain: 0.25, pan });
  }

  click(at: number, pan: number) {
    this.track.noise(at, { dur: 0.018, gain: 0.34, pan, decay: 8, lowpass: 9000 });
    this.track.tone(at, { freq: 2600, to: 1300, dur: 0.035, gain: 0.24, wave: "triangle", pan });
  }

  pop(at: number, pan = 0, pitch = 1) {
    this.track.tone(at, { freq: 240 * pitch, to: 820 * pitch, dur: 0.12, gain: 0.5, pan, decay: 4 });
    this.track.tone(at, { freq: 480 * pitch, to: 1640 * pitch, dur: 0.08, gain: 0.12, wave: "triangle", pan });
  }

  blip(at: number, note: number, pan = 0, gain = 0.13) {
    this.track.tone(at, { freq: midi(note), dur: 0.09, gain, wave: "square", pan, lowpass: 3800, decay: 4 });
    this.track.tone(at + 0.05, { freq: midi(note + 7), dur: 0.12, gain: gain * 0.8, wave: "square", pan, lowpass: 3800, decay: 4 });
  }

  tick(at: number, freq: number, pan = 0, gain = 0.06) {
    this.track.tone(at, { freq, dur: 0.014, gain, wave: "square", pan, lowpass: 6000 });
  }

  ding(at: number, note: number, pan = 0) {
    this.track.tone(at, { freq: midi(note), dur: 0.4, gain: 0.2, wave: "triangle", pan, decay: 5 });
    this.track.tone(at, { freq: midi(note + 12), dur: 0.25, gain: 0.06, pan, decay: 6 });
  }

  chime(at: number, root: number, pan = 0) {
    [0, 4, 7, 12].forEach((step, i) => {
      this.track.tone(at + i * 0.065, { freq: midi(root + step), dur: 0.7, gain: 0.2, wave: "triangle", pan, decay: 4 });
      this.track.tone(at + i * 0.065, { freq: midi(root + step + 12), dur: 0.45, gain: 0.05, pan, decay: 5 });
    });
  }

  swish(at: number, dur: number, pan = 0, gain = 0.22) {
    this.track.noise(at, { dur, gain, pan, attack: dur * 0.45, decay: 2, lowpass: [500, 6500] });
  }

  /** Two decorrelated noises, one each side, for a wide rush into the wipe. */
  whoosh(at: number, dur: number) {
    for (const pan of [-0.8, 0.8]) this.track.noise(at, { dur, gain: 0.3, pan, attack: dur * 0.55, decay: 1.5, lowpass: [350, 7000] });
    this.track.tone(at, { freq: 90, to: 180, dur, gain: 0.12, attack: dur * 0.5, decay: 2 });
  }

  thud(at: number, pan = 0, gain = 0.9) {
    this.track.tone(at, { freq: 150, to: 40, dur: 0.34, gain, pan, decay: 4 });
    this.track.noise(at, { dur: 0.12, gain: gain * 0.45, pan, decay: 6, lowpass: 420 });
  }

  boing(at: number, pan = 0, gain = 0.2) {
    this.track.tone(at, { freq: 220, to: 560, dur: 0.16, gain, wave: "triangle", pan, decay: 3 });
  }

  fall(at: number, dur: number) {
    this.track.tone(at, { freq: 1500, to: 260, dur, gain: 0.12, attack: 0.05, decay: 0.5 });
  }

  buzz(at: number, pan: number) {
    this.track.tone(at, { freq: 150, dur: 0.15, gain: 0.24, wave: "square", pan, lowpass: 900, decay: 0.4, attack: 0.008 });
    this.track.tone(at, { freq: 75, dur: 0.15, gain: 0.3, pan, decay: 0.4, attack: 0.008 });
  }

  glitch(at: number, dur: number, pan: number) {
    for (let i = 0; i < 12; i++) {
      const r = this.track.rand();
      this.track.tone(at + (i / 12) * dur, { freq: 180 + r * 1800, dur: 0.028, gain: 0.14, wave: "square", pan: pan + (r - 0.5) * 0.6, lowpass: 5000 });
    }
    this.track.noise(at, { dur, gain: 0.2, pan, decay: 3, lowpass: 3000 });
  }

  error(at: number, pan: number) {
    this.track.tone(at, { freq: 233, dur: 0.12, gain: 0.2, wave: "square", pan, lowpass: 2000, decay: 1 });
    this.track.tone(at + 0.13, { freq: 175, dur: 0.2, gain: 0.2, wave: "square", pan, lowpass: 2000, decay: 2 });
  }

  powerdown(at: number) {
    this.track.tone(at, { freq: 560, to: 36, dur: 1.0, gain: 0.24, wave: "saw", lowpass: [3000, 300], decay: 1.5 });
  }

  jingle(at: number) {
    [67, 72, 76, 79, 84].forEach((n, i) => this.track.tone(at + i * 0.09, { freq: midi(n), dur: 1.5, gain: 0.17, wave: "triangle", decay: 3.5 }));
    for (const n of [60, 64, 67]) this.track.tone(at + 0.4, { freq: midi(n), dur: 1.6, gain: 0.08, attack: 0.2, decay: 2 });
    this.track.tone(at, { freq: midi(48), dur: 1.4, gain: 0.22, decay: 3 });
  }

  sting(at: number) {
    [72, 76, 79, 84, 88, 91].forEach((n, i) => this.track.tone(at + i * 0.06, { freq: midi(n), dur: 1.1, gain: 0.15, wave: "triangle", pan: -0.5 + i * 0.2, decay: 3.5 }));
    this.track.tone(at, { freq: midi(48), dur: 1.2, gain: 0.25, decay: 3 });
  }
}

/** The tour's sound effects as two channels at `RATE`. */
export function tourSfx(tour: readonly Scene[], seconds: number): { left: Float32Array; right: Float32Array } {
  const track = new Track(seconds);
  const f = new Foley(track);
  const at = (id: Scene["id"]) => {
    const scene = tour.find((s) => s.id === id);
    if (!scene) throw new Error(`no ${id} scene`);
    return scene.start;
  };

  tour.slice(0, -1).forEach((s) => f.whoosh(s.end - WIPE, WIPE * 1.6));

  {
    const c = CUES.boot;
    const s = at("boot");
    f.fall(s + c.drop, c.land - c.drop);
    f.thud(s + c.land);
    [..."Owlhead 98"].forEach((ch, i) => {
      if (ch !== " ") f.tick(s + c.letters + i * c.letterGap + 0.06, midi(72 + PENTATONIC[i]), (i - 4.5) / 7, 0.09);
    });
    for (let b = 1; b <= c.bars; b++) f.tick(s + c.bar[0] + (b / c.bars) * (c.bar[1] - c.bar[0]), 900 + b * 30, -0.5 + b / c.bars, 0.045);
    f.jingle(s + c.jingle);
    f.boing(s + c.jingle);
  }

  {
    const c = CUES.rules;
    const s = at("rules");
    f.pop(s + c.win, panAt(385));
    [...RULES_TEXT].forEach((ch, i) => {
      const when = s + c.typeFrom + (i + 1) / c.typeRate;
      if (ch === "\n") f.carriage(when, panAt(385));
      else if (ch !== " ") f.key(when, panAt(120 + (i % 36) * 15));
    });
    f.pop(s + c.dialog, panAt(950), 1.2);
    f.click(s + c.press, panAt(1074));
    f.chime(s + c.press + 0.05, 72, panAt(1036));
  }

  {
    const c = CUES.ideas;
    const s = at("ideas");
    f.pop(s + c.win, 0);
    for (let i = 0; i < 4; i++) {
      const when = s + c.row0 + i * c.rowGap;
      f.swish(when - 0.08, 0.2, panAt(700));
      f.blip(when + 0.05, 69 + PENTATONIC[i * 2], panAt(700));
      f.boing(when, panAt(260), 0.12);
    }
  }

  {
    const c = CUES.orders;
    const s = at("orders");
    for (let i = 0; i < 4; i++) {
      f.blip(s + c.step0 + i * c.stepGap, 64 + PENTATONIC[i * 2], panAt(290));
      if (i > 0 && i < 3) f.ding(s + c.step0 + i * c.stepGap + 0.12, 84 + i * 2, panAt(470));
    }
    f.swish(s + c.phone - 0.05, 0.42, panAt(925), 0.3);
    f.pop(s + c.phone + 0.25, panAt(925), 0.8);
    f.buzz(s + c.buzz, panAt(925));
    f.buzz(s + c.buzz + 0.3, panAt(925));
    f.click(s + c.press, panAt(896));
    f.chime(s + c.approved, 76, panAt(925));
    f.pop(s + c.approved, panAt(925), 1.4);
  }

  {
    const c = CUES.record;
    const s = at("record");
    f.pop(s + c.win, 0);
    for (let i = 0; i < 7; i++) {
      const row = s + c.row0 + i * c.rowGap;
      for (let j = 0; j < 5; j++) f.tick(row + j * 0.06, 1400 + track.rand() * 1600, panAt(1050), 0.04);
      f.tick(row + 0.32, midi(88), panAt(1050), 0.08);
    }
    f.click(s + c.click, panAt(402));
    f.glitch(s + c.tamper, 0.3, panAt(402));
    f.thud(s + c.tamper, panAt(402), 0.6);
    f.error(s + c.tamper + 0.05, panAt(402));
    for (let i = 2; i < 7; i++) f.blip(s + c.tamper + 0.15 + (i - 2) * c.cascadeGap, 52 - i * 2, panAt(1050), 0.11);
  }

  {
    const c = CUES.check;
    const s = at("check");
    f.pop(s + c.win, 0);
    for (let i = 0; i < 3; i++) {
      const when = s + c.item0 + i * c.itemGap;
      f.swish(when - 0.06, 0.22, panAt(300));
      f.ding(when + 0.1, 79 + PENTATONIC[i], panAt(250));
    }
    f.pop(s + c.stop, panAt(240), 0.7);
    f.click(s + c.press, panAt(288));
    f.thud(s + c.press, panAt(288), 1);
    f.powerdown(s + c.press + 0.05);
    for (let i = 0; i < 3; i++) f.tick(s + c.press + i * 0.09, midi(60 - i * 3), panAt(870 + i * 114), 0.1);
  }

  {
    const c = CUES.end;
    const s = at("end");
    f.pop(s + c.win, 0, 0.9);
    f.sting(s + c.sting);
    f.boing(s + c.sting, panAt(450));
    f.boing(s + c.sting + 0.36, panAt(450), 0.14);
    for (let i = 0; i < 5; i++) f.pop(s + c.perch0 + i * c.perchGap, panAt(500 + i * 76), 1.2 + PENTATONIC[i] / 12);
  }

  return { left: track.left, right: track.right };
}

/** Two channels as a 32-bit float WAV, so nothing clips before the mix is levelled. */
export function wav({ left, right }: { left: Float32Array; right: Float32Array }): Buffer {
  const bytes = left.length * 8;
  const out = Buffer.alloc(44 + bytes);
  out.write("RIFF", 0);
  out.writeUInt32LE(36 + bytes, 4);
  out.write("WAVE", 8);
  out.write("fmt ", 12);
  out.writeUInt32LE(16, 16);
  out.writeUInt16LE(3, 20);
  out.writeUInt16LE(2, 22);
  out.writeUInt32LE(RATE, 24);
  out.writeUInt32LE(RATE * 8, 28);
  out.writeUInt16LE(8, 32);
  out.writeUInt16LE(32, 34);
  out.write("data", 36);
  out.writeUInt32LE(bytes, 40);
  for (let i = 0; i < left.length; i++) {
    out.writeFloatLE(left[i], 44 + i * 8);
    out.writeFloatLE(right[i], 48 + i * 8);
  }
  return out;
}
