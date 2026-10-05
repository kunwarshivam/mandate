/**
 * The tour's synthesizer: plain oscillators and seeded noise written into a stereo buffer, so the
 * music (`tour-music.ts`) and the effects (`tour-sound.ts`) render the same on every run.
 */
export const RATE = 48_000;

export type Wave = "sine" | "square" | "triangle" | "saw" | "pulse";

export interface Voice {
  dur: number;
  gain: number;
  /** -1 left to 1 right. */
  pan?: number;
  attack?: number;
  /** How fast it dies away: higher is shorter. */
  decay?: number;
  /** A one-pole low-pass, in Hz, to soften a square's edge or darken a noise; a pair sweeps it. */
  lowpass?: number | [number, number];
  /** A one-pole high-pass, in Hz, to thin a noise into a hat or a snare. */
  highpass?: number;
}

export interface Tone extends Voice {
  freq: number;
  /** The pitch it glides to by the end, exponentially. */
  to?: number;
  wave?: Wave;
  /** The pulse wave's duty cycle: 0.5 is a square, 0.125 the thin chip lead. */
  duty?: number;
  /** A wobble that fades in after `after` seconds, `depth` semitones either way. */
  vibrato?: { rate: number; depth: number; after: number };
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

export const midi = (n: number) => 440 * 2 ** ((n - 69) / 12);

export const clamp = (v: number, a: number, b: number) => Math.min(b, Math.max(a, v));

function oscillate(wave: Wave, cycles: number, duty: number): number {
  const p = cycles - Math.floor(cycles);
  switch (wave) {
    case "sine":
      return Math.sin(2 * Math.PI * p);
    case "square":
      return p < 0.5 ? 1 : -1;
    case "pulse":
      return p < duty ? 1 : -1;
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

const pole = (fc: number) => 1 - Math.exp((-2 * Math.PI * fc) / RATE);

export class Track {
  readonly left: Float32Array;
  readonly right: Float32Array;
  private readonly random: () => number;

  constructor(seconds: number, seed: number) {
    this.left = new Float32Array(Math.ceil(seconds * RATE));
    this.right = new Float32Array(this.left.length);
    this.random = generator(seed);
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
    const hp = v.highpass ? pole(v.highpass) : 0;
    let low = 0;
    let rumble = 0;
    for (let i = 0; i < n && start + i < this.left.length; i++) {
      if (start + i < 0) continue;
      const t = i / RATE;
      const k = i / n;
      const env = Math.min(1, t / attack) * Math.exp(-decay * k) * (1 - k);
      let x = source(t);
      if (hp > 0) {
        rumble += hp * (x - rumble);
        x -= rumble;
      }
      if (lpFrom > 0) {
        low += pole(lpFrom * (lpTo / lpFrom) ** k) * (x - low);
        x = low;
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
      const wobble = v.vibrato ? 2 ** ((v.vibrato.depth * Math.sin(2 * Math.PI * v.vibrato.rate * t) * clamp((t - v.vibrato.after) / 0.12, 0, 1)) / 12) : 1;
      cycles += v.freq * ratio ** (t / v.dur) * wobble * (t - last);
      last = t;
      return oscillate(wave, cycles, v.duty ?? 0.5);
    });
  }

  noise(at: number, v: Voice) {
    this.voice(at, v, () => this.random() * 2 - 1);
  }
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
