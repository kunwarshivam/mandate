/**
 * The pixel owl's sprite (DEC-217), with no React in it, so the `Owl` component and the landing
 * tour's renderer (`scripts/render-tour.ts`) draw the very same owl from the very same pixels.
 */
export type OwlMood = "awake" | "focused" | "asleep" | "stopped";

/** FNV-1a: the same ID always draws the same owl. */
export function owlSeed(id: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

export const FEATHERS = ["var(--series-1)", "var(--series-2)", "var(--series-3)", "var(--series-4)"] as const;
const EARS = ["tufts", "wide", "round"] as const;
const MARKING = ["none", "speckles", "scallops"] as const;
const BLINK_EVERY = [3.8, 4.6, 5.4, 6.2] as const;

export interface OwlShape {
  feathers: (typeof FEATHERS)[number];
  ears: (typeof EARS)[number];
  marking: (typeof MARKING)[number];
  /** Seconds between blinks, and how far into that cycle this owl starts, so owls never blink together. */
  blink: { every: number; offset: number };
}

export function owlShape(id: string): OwlShape {
  const s = owlSeed(id);
  const pick = <T>(list: readonly T[], shift: number) => list[(s >>> shift) % list.length];
  const every = pick(BLINK_EVERY, 20);
  return {
    feathers: pick(FEATHERS, 11),
    ears: pick(EARS, 3),
    marking: pick(MARKING, 14),
    blink: { every, offset: Math.round((((s >>> 23) % 100) / 100) * every * 10) / 10 },
  };
}

/** One sprite pixel, in viewBox units. */
export const PX = 4;

/**
 * The sprite's legend: o outline, b feathers, d wing (feathers in shade), l belly (feathers in
 * light), w eye white, k beak, K the beak's shaded tip, f feet.
 */
type Ink = "o" | "b" | "d" | "l" | "w" | "k" | "K" | "f";

/** The ears and the crown, rows 0 to 2. */
const EAR_ROWS: Record<OwlShape["ears"], readonly [string, string, string]> = {
  tufts: [".o............o.", ".obo........obo.", ".obboooooooobbo."],
  wide: ["oo............oo", "obbo........obbo", "obbboooooooobbbo"],
  round: ["................", "..oooooooooooo..", ".obbbbbbbbbbbbo."],
};

/** Rows 3 to 15: round eyes, the beak between them, folded wings, a pale belly, and the feet. */
const BODY = [
  "obbbbbbbbbbbbbbo",
  "obbwwbbbbbbwwbbo",
  "obwwwwbbbbwwwwbo",
  "odwwwwbkkbwwwwdo",
  "odbwwbbKKbbwwbdo",
  "oddbbbbbbbbbbddo",
  "oddbbllllllbbddo",
  "oddbllllllllbddo",
  ".oddbllllllbddo.",
  "..oddbllllbddo..",
  "...oooooooooo...",
  "....ff....ff....",
  "................",
] as const;

/** The whole sprite for a set of ears, 16 rows of 16. */
export function owlRows(ears: OwlShape["ears"]): readonly string[] {
  return [...EAR_ROWS[ears], ...BODY];
}

/**
 * Feather marks on the belly, m. Scattered or even, never a curve: a curve under the beak reads as a
 * mouth, and an owl's face says its mode and nothing else.
 */
const MARKING_ROWS: Record<OwlShape["marking"], Record<number, string>> = {
  none: {},
  speckles: { 10: "....m...m.......", 11: "..........m.....", 12: "......m........." },
  scallops: { 10: "....m.m.m.m.....", 11: ".....m.m.m......" },
};

/** The two eyes start at these columns, on rows 4 to 7. */
const EYE_COLS = [2, 10] as const;
export const EYE_ROW = 4;

/** The beak is sun, except on a sun owl, where it is the pupil's colour so it still reads. */
export function beakFor(feathers: string): string {
  return feathers === "var(--series-2)" ? "var(--owl-pupil)" : "var(--highlight)";
}

/** Over the base colours: the wings and the beak's tip in shade, the belly in light. */
const SHADE: Partial<Record<Ink, number>> = { d: 0.24, K: 0.28 };
const LIGHT: Partial<Record<Ink, number>> = { l: 0.42 };

/** A filled block of the sprite, in sprite pixels. */
export interface OwlRect {
  x: number;
  y: number;
  w: number;
  h: number;
  fill: string;
  opacity?: number;
}

function pixels(rows: Record<number, string>, fill: (c: string) => string | null, opacity?: (c: string) => number | undefined): OwlRect[] {
  return Object.entries(rows).flatMap(([y, row]) =>
    [...row].flatMap((c, x) => {
      const f = fill(c);
      return f ? [{ x, y: Number(y), w: 1, h: 1, fill: f, opacity: opacity?.(c) }] : [];
    }),
  );
}

/** The owl without its eyes: feathers, then the shade, the light and the belly marks over them. */
export function bodyRects(shape: OwlShape, feathers: string, beak: string): OwlRect[] {
  const base: Record<Ink, string> = { o: "var(--owl-line)", b: feathers, d: feathers, l: feathers, w: "var(--owl-eye)", k: beak, K: beak, f: beak };
  const head = Object.fromEntries(owlRows(shape.ears).map((row, y) => [y, row]));
  return [
    ...pixels(head, (c) => base[c as Ink] ?? null),
    ...pixels(head, (c) => (SHADE[c as Ink] ? "var(--owl-line)" : null), (c) => SHADE[c as Ink]),
    ...pixels(head, (c) => (LIGHT[c as Ink] ? "var(--owl-eye)" : null), (c) => LIGHT[c as Ink]),
    ...pixels(MARKING_ROWS[shape.marking], (c) => (c === "m" ? "var(--owl-line)" : null), () => 0.3),
  ];
}

/**
 * The eyes in a mood: `open` holds each open eye's pupil and glint, which blink and follow a gaze;
 * `fixed` holds the lids and closed-eye lines, which never move.
 */
export function eyeRects(mood: OwlMood, feathers: string): { open: OwlRect[][]; fixed: OwlRect[] } {
  const lid = (h: number) => EYE_COLS.map((x) => ({ x, y: EYE_ROW, w: 4, h, fill: feathers }));
  const ink = (x: number, y: number, w: number) => ({ x, y, w, h: 1, fill: "var(--owl-pupil)" });
  switch (mood) {
    case "awake":
    case "focused": {
      const dy = mood === "focused" ? 1 : 0;
      const open = EYE_COLS.map((x) => [
        { x: x + 1, y: EYE_ROW + 1 + dy, w: 2, h: 2, fill: "var(--owl-pupil)" },
        { x: x + 1, y: EYE_ROW + 1 + dy, w: 1, h: 1, fill: "var(--owl-eye)" },
      ]);
      return { open, fixed: mood === "focused" ? lid(2) : [] };
    }
    case "asleep":
      return { open: [], fixed: [...lid(4), ...EYE_COLS.flatMap((x) => [ink(x, EYE_ROW + 1, 1), ink(x + 3, EYE_ROW + 1, 1), ink(x + 1, EYE_ROW + 2, 2)])] };
    case "stopped":
      return { open: [], fixed: [...lid(4), ...EYE_COLS.map((x) => ink(x, EYE_ROW + 2, 4))] };
    default: {
      const unhandled: never = mood;
      throw new Error(`unhandled owl mood ${String(unhandled)}`);
    }
  }
}
