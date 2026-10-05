/**
 * The tour's music: an original chiptune in C major at 120 beats a minute, written for the tour and
 * cut to it. Each scene is a section that starts on its own downbeat, the presses, pops and rows
 * fall on its beats (`tour-cues.ts`), and when Stop is pressed the band stops with the agents.
 *
 * Voices, as a 1990s sound chip had them: a pulse lead with an echo, a thin pulse arpeggio, a
 * triangle bass, and noise and sine drums.
 */
import type { Scene } from "../src/components/site/tour.ts";
import { BEAT, CUES } from "./tour-cues.ts";
import { Track, midi } from "./tour-synth.ts";

type Chord = "C" | "G" | "Am" | "F" | "E";

/** Each chord's bass root, and its third in semitones. */
const CHORDS: Record<Chord, { root: number; third: number }> = {
  C: { root: 48, third: 4 },
  G: { root: 43, third: 4 },
  Am: { root: 45, third: 3 },
  F: { root: 41, third: 4 },
  E: { root: 40, third: 4 },
};

/** A tune as [note, eighths]. */
type Line = readonly (readonly [number, number])[];

/** The theme, a bar for each chord of C, G, Am, F. */
const THEME: Record<"C" | "G" | "Am" | "F", Line> = {
  C: [[76, 1], [79, 1], [84, 2], [83, 1], [79, 1], [76, 2]],
  G: [[74, 1], [79, 1], [83, 2], [81, 1], [79, 1], [74, 2]],
  Am: [[72, 1], [76, 1], [81, 2], [79, 1], [76, 1], [72, 1], [76, 1]],
  F: [[77, 2], [81, 1], [79, 1], [77, 1], [76, 1], [74, 2]],
};

type Groove = "light" | "full" | "half";

/** Which eighths of a bar each drum plays on. */
const GROOVES: Record<Groove, { kick: number[]; snare: number[]; hat: number[]; open: number[] }> = {
  light: { kick: [0, 4], snare: [2, 6], hat: [0, 1, 2, 3, 4, 5, 6, 7], open: [] },
  full: { kick: [0, 3, 4], snare: [2, 6], hat: [0, 1, 2, 3, 4, 5, 6], open: [7] },
  half: { kick: [0], snare: [4], hat: [0, 2, 4, 6], open: [] },
};

/** The bass's eighths, in semitones over the root: the octave bounce, with a fifth to turn the bar. */
const BOUNCE = [0, 12, 0, 12, 0, 12, 7, 12];

/** The track, written in beats. */
class Band {
  readonly track: Track;

  constructor(track: Track) {
    this.track = track;
  }

  kick(b: number, gain = 1) {
    this.track.tone(b * BEAT, { freq: 150, to: 48, dur: 0.16, gain: 0.8 * gain, decay: 3 });
    this.track.noise(b * BEAT, { dur: 0.008, gain: 0.25 * gain, lowpass: 2500 });
  }

  snare(b: number, gain = 1) {
    this.track.noise(b * BEAT, { dur: 0.14, gain: 0.34 * gain, highpass: 1500, decay: 4 });
    this.track.tone(b * BEAT, { freq: 220, to: 170, dur: 0.07, gain: 0.22 * gain, wave: "triangle" });
  }

  hat(b: number, open: boolean, gain = 1) {
    this.track.noise(b * BEAT, { dur: open ? 0.14 : 0.03, gain: (open ? 0.07 : 0.09) * gain, highpass: 8000, decay: open ? 3 : 6, pan: 0.25 });
  }

  crash(b: number) {
    for (const pan of [-0.5, 0.5]) this.track.noise(b * BEAT, { dur: 1.4, gain: 0.15, highpass: 5000, decay: 3, pan });
  }

  drums(b: number, beats: number, groove: Groove) {
    const g = GROOVES[groove];
    for (let e = 0; e < beats * 2; e++) {
      const at = b + e / 2;
      const step = e % 8;
      if (g.kick.includes(step)) this.kick(at);
      if (g.snare.includes(step)) this.snare(at, groove === "light" ? 0.7 : 1);
      if (g.hat.includes(step)) this.hat(at, false, e % 2 ? 0.6 : 1);
      if (g.open.includes(step)) this.hat(at, true);
    }
  }

  /** Snares on every sixteenth, rising from `from` to `to`. */
  roll(b: number, beats: number, from = 0.25, to = 1) {
    const n = beats * 4;
    for (let i = 0; i < n; i++) this.snare(b + i / 4, from + ((to - from) * i) / Math.max(1, n - 1));
  }

  bass(b: number, chord: Chord, beats: number) {
    const { root } = CHORDS[chord];
    for (let e = 0; e < beats * 2; e++) {
      const freq = midi(root + BOUNCE[e % 8]);
      const at = (b + e / 2) * BEAT;
      this.track.tone(at, { freq, dur: 0.22, gain: 0.32, wave: "triangle", decay: 1.5 });
      this.track.tone(at, { freq, dur: 0.2, gain: 0.07, wave: "pulse", lowpass: 1000, decay: 2 });
    }
  }

  /** The chord's notes a sixteenth apiece, up and over, swelling from `from` to `to`. */
  arp(b: number, chord: Chord, beats: number, from = 1, to = from) {
    const { root, third } = CHORDS[chord];
    const notes = [0, third, 7, 12].map((n) => root + 12 + n);
    const n = beats * 4;
    for (let i = 0; i < n; i++) {
      const gain = 0.05 * (from + ((to - from) * i) / Math.max(1, n - 1));
      this.track.tone((b + i / 4) * BEAT, { freq: midi(notes[i % 4]), dur: 0.1, gain, wave: "pulse", duty: 0.125, lowpass: 7000, decay: 2.5, pan: i % 2 ? 0.3 : -0.3 });
    }
  }

  pad(b: number, chord: Chord, beats: number, gain = 1) {
    const { root, third } = CHORDS[chord];
    for (const n of [0, third, 7]) this.track.tone(b * BEAT, { freq: midi(root + 12 + n), dur: beats * BEAT, gain: 0.035 * gain, wave: "triangle", attack: Math.min(0.4, beats * BEAT * 0.3), decay: 0.8 });
  }

  /** A tune on the pulse lead, with an echo three sixteenths behind it, none past `until`. */
  lead(b: number, line: Line, until = Infinity) {
    let at = b;
    for (const [note, eighths] of line) {
      const dur = eighths * 0.5 * BEAT * 0.92;
      const voice = { freq: midi(note), wave: "pulse" as const, duty: 0.25, lowpass: 6000, decay: 1, vibrato: { rate: 5.5, depth: 0.2, after: 0.15 } };
      this.track.tone(at * BEAT, { ...voice, dur, gain: 0.12, pan: -0.15 });
      if (at + 0.75 + eighths / 2 <= until) this.track.tone((at + 0.75) * BEAT, { ...voice, dur, gain: 0.045, pan: 0.45 });
      at += eighths / 2;
    }
  }

  /** Long notes, one a chord, `beats` apiece. */
  hold(b: number, notes: readonly (readonly [number, number])[]) {
    let at = b;
    for (const [note, beats] of notes) {
      this.track.tone(at * BEAT, { freq: midi(note), dur: beats * BEAT * 0.95, gain: 0.11, wave: "pulse", duty: 0.25, lowpass: 5000, attack: 0.02, decay: 0.7, pan: -0.15, vibrato: { rate: 5, depth: 0.25, after: 0.25 } });
      at += beats;
    }
  }

  /** Bars of one chord each: bass and arpeggio, and drums when given a groove. */
  bars(b: number, chords: readonly (readonly [Chord, number])[], groove?: Groove) {
    let at = b;
    for (const [chord, beats] of chords) {
      this.bass(at, chord, beats);
      this.arp(at, chord, beats, 0.85);
      if (groove) this.drums(at, beats, groove);
      at += beats;
    }
  }
}

/** The music as two channels at `RATE`. */
export function tourMusic(tour: readonly Scene[], seconds: number): { left: Float32Array; right: Float32Array } {
  const band = new Band(new Track(seconds, 0x5eed98));
  const start = (id: Scene["id"]) => {
    const scene = tour.find((s) => s.id === id);
    if (!scene) throw new Error(`no ${id} scene`);
    return scene.start / BEAT;
  };

  {
    const s = start("boot");
    const jingle = s + CUES.boot.jingle / BEAT;
    band.pad(s + 2, "F", 4, 1.6);
    band.arp(s + 2, "F", 4, 0.6, 1.1);
    band.pad(s + 6, "G", jingle - s - 6, 1.8);
    band.arp(s + 6, "G", jingle - s - 6, 1.1, 1.4);
    for (const [chord, at, beats] of [["F", s + 2, 4], ["G", s + 6, jingle - s - 6]] as const) band.track.tone(at * BEAT, { freq: midi(CHORDS[chord].root), dur: beats * BEAT, gain: 0.3, wave: "triangle", attack: 0.05, decay: 0.6 });
    for (let b = s + 4; b < jingle; b++) band.kick(b, 0.4 + (0.5 * (b - s - 4)) / (jingle - s - 4));
    band.crash(jingle);
    band.roll(jingle + 1, 1, 0.15, 0.9);
  }

  {
    const s = start("rules");
    band.bars(s, [["Am", 4], ["F", 4], ["G", 4], ["C", 4], ["G", 4]], "light");
    band.roll(s + 18.5, 1.5, 0.3, 0.8);
  }

  {
    const s = start("ideas");
    band.crash(s);
    band.bars(s, [["C", 4], ["G", 4], ["Am", 4], ["F", 4]], "full");
    (["C", "G", "Am", "F"] as const).forEach((c, i) => band.lead(s + i * 4, THEME[c]));
  }

  {
    const s = start("orders");
    const approved = s + CUES.orders.approved / BEAT;
    band.crash(s);
    band.bars(s, [["F", 4], ["G", 4], ["Am", 4]], "full");
    band.bars(s + 12, [["G", approved - s - 12]]);
    band.kick(s + 12);
    band.kick(s + 13);
    band.roll(s + 12, approved - s - 12, 0.3, 1);
    band.crash(approved);
    band.bars(approved, [["C", 4]], "full");
    band.bars(approved + 4, [["G", 2]]);
    band.roll(approved + 4.5, 1.5, 0.3, 0.9);
    band.hold(s, [[81, 4], [83, 4], [84, 4], [86, approved - s - 12], [88, 4], [86, 2]]);
  }

  {
    const s = start("record");
    const tamper = s + CUES.record.tamper / BEAT;
    band.bars(s, [["Am", 4], ["F", 4], ["E", tamper - s - 8]], "light");
    band.pad(s, "Am", 4, 0.8);
    band.pad(s + 4, "F", 4, 0.8);
    band.pad(s + 8, "E", tamper - s - 8, 1);
    for (let i = 0; i < 8; i++) band.track.tone((tamper + i / 4) * BEAT, { freq: midi(40 + (i % 2) * 13), dur: 0.1, gain: 0.2 * (1 - i / 10), wave: "pulse", duty: 0.5, lowpass: 1800, decay: 2 });
    const after = tamper + 2;
    band.bass(after, "Am", 4);
    band.pad(after, "Am", 4, 1.2);
    band.drums(after, 4, "half");
    band.roll(after + 3, 1, 0.3, 0.8);
  }

  {
    const s = start("check");
    const press = s + CUES.check.press / BEAT;
    band.crash(s);
    band.bars(s, [["C", 4], ["G", press - s - 5]], "full");
    band.bars(press - 1, [["G", 1]]);
    band.roll(press - 1, 1, 0.4, 1);
    band.lead(s, THEME.C, press);
    band.lead(s + 4, THEME.G, press);
    band.pad(press + 3, "C", start("end") - press - 3, 1.3);
  }

  {
    const s = start("end");
    const last = s + 12;
    band.crash(s);
    band.bars(s, [["C", 4], ["Am", 4], ["F", 2], ["G", 2]], "full");
    band.lead(s, THEME.C);
    band.lead(s + 4, THEME.Am);
    band.lead(s + 8, [[77, 2], [81, 1], [79, 1], [74, 1], [79, 1], [83, 1], [86, 1]]);
    band.crash(last);
    band.kick(last, 1.2);
    band.track.tone(last * BEAT, { freq: midi(84), dur: 2 * BEAT, gain: 0.13, wave: "pulse", duty: 0.25, lowpass: 6000, decay: 2.5, pan: -0.15 });
    band.track.tone(last * BEAT, { freq: midi(36), dur: 2 * BEAT, gain: 0.4, wave: "triangle", decay: 2.5 });
    band.pad(last, "C", 2, 1.5);
  }

  return { left: band.track.left, right: band.track.right };
}
