import { OWL_ROWS } from "./voxel-owl";
import styles from "./scroll.module.css";

/**
 * The lock screen's wallpaper (DEC-907): a night in pixels, the same in both themes, as the
 * desktop's paintings are moonlight. A dark sky dithered down into tide, stars, a full moon, and the
 * brand owl awake on a branch over the hills, its face ringed in starlight and its sun eyes the only
 * warm thing in it. Every pixel is a flat token colour; the sky's fall into tide is dithering, never
 * a blend.
 */

export const COLS = 44;
export const ROWS = 95;

export type NightInk = "sky" | "tide" | "star" | "moon" | "land" | "eye" | "glint";

export const NIGHT_COLOURS: Record<NightInk, string> = {
  sky: "var(--highlight-foreground)",
  tide: "var(--tide)",
  star: "var(--tide-muted)",
  moon: "var(--highlight)",
  land: "var(--highlight-foreground)",
  eye: "var(--highlight)",
  glint: "var(--tide-foreground)",
};

/** Where the sky has fallen all the way into tide; the rows above it dither in 2 by 2 steps. */
const HORIZON = 62;
const DITHER_ROWS = 4;
const BAYER = [
  [0, 2],
  [3, 1],
] as const;
const MOON = { x: 10.5, y: 55.5, r: 4.8 };
const OWL = { x: 25, y: 64 };
const BRANCH_ROW = OWL.y + OWL_ROWS.length;

/** The stars, as column and row; the ones in `TWINKLE` blink now and then. */
export const STARS: readonly (readonly [number, number])[] = [
  [3, 3], [17, 2], [29, 5], [40, 2], [8, 9], [36, 11], [2, 17], [41, 19], [22, 26], [5, 27],
  [38, 28], [14, 31], [31, 34], [2, 38], [42, 40], [24, 43], [35, 47], [19, 50], [41, 53], [29, 56],
];
const TWINKLE = new Set([1, 4, 8, 12, 15, 18]);

function ridge(x: number): number {
  return Math.round(90 + 2.2 * Math.sin(x / 6.5 + 0.6) + 1.2 * Math.sin(x / 2.7));
}

function inkAt(x: number, y: number): NightInk {
  const row = OWL_ROWS[y - OWL.y];
  const c = row?.[x - OWL.x];
  if (c && c !== ".") return c === "y" ? "eye" : c === "g" ? "glint" : c === "r" ? "star" : "land";
  if (y >= BRANCH_ROW && y <= BRANCH_ROW + 1 && x >= OWL.x - 3) return "land";
  if (y >= BRANCH_ROW - 2 && y < BRANCH_ROW && x === OWL.x - 2) return "land";
  if (y >= ridge(x)) return "land";
  if ((x + 0.5 - MOON.x) ** 2 + (y + 0.5 - MOON.y) ** 2 <= MOON.r ** 2) return "moon";
  if (y >= HORIZON) return "tide";
  const step = HORIZON - y;
  if (step <= DITHER_ROWS * 3) {
    const level = 3 - Math.floor((step - 1) / DITHER_ROWS);
    if (BAYER[y % 2]![x % 2]! < level) return "tide";
  }
  return "sky";
}

export interface NightRect {
  ink: NightInk;
  x: number;
  y: number;
  w: number;
  twinkle?: number;
}

/** The night as runs of one ink along each row, over the sky and tide laid down as two blocks. */
export function nightRects(): NightRect[] {
  const stars = new Map(STARS.map(([x, y], i) => [`${x},${y}`, i]));
  const rects: NightRect[] = [];
  for (let y = 0; y < ROWS; y++) {
    const base: NightInk = y >= HORIZON ? "tide" : "sky";
    let x = 0;
    while (x < COLS) {
      const star = stars.get(`${x},${y}`);
      const ink = inkAt(x, y);
      if (star !== undefined && ink === "sky") {
        rects.push({ ink: "star", x, y, w: 1, twinkle: TWINKLE.has(star) ? star : undefined });
        x++;
        continue;
      }
      let end = x + 1;
      while (end < COLS && inkAt(end, y) === ink && !stars.has(`${end},${y}`)) end++;
      if (ink !== base) rects.push({ ink, x, y, w: end - x });
      x = end;
    }
  }
  return rects;
}

const RECTS = nightRects();

export function PixelNight({ className }: { className?: string }) {
  return (
    <svg aria-hidden viewBox={`0 0 ${COLS} ${ROWS}`} preserveAspectRatio="xMidYMid slice" shapeRendering="crispEdges" className={className} data-slot="pixel-night" data-ambient="">
      <rect width={COLS} height={HORIZON} fill={NIGHT_COLOURS.sky} />
      <rect y={HORIZON} width={COLS} height={ROWS - HORIZON} fill={NIGHT_COLOURS.tide} />
      {RECTS.map((r) => (
        <rect
          key={`${r.x},${r.y}`}
          x={r.x}
          y={r.y}
          width={r.w}
          height={1}
          fill={NIGHT_COLOURS[r.ink]}
          className={r.twinkle !== undefined ? styles.twinkle : undefined}
          style={r.twinkle !== undefined ? { animationDelay: `${-(r.twinkle * 0.7) % 3.1}s` } : undefined}
        />
      ))}
    </svg>
  );
}
