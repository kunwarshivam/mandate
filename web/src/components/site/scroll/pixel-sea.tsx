import type { CSSProperties } from "react";
import styles from "./scroll.module.css";

/**
 * The opening's sea (DEC-908, DEC-932): a tide sea in pixels runs behind the agent's page from just
 * above it to below it. The water is rows of curling crests, small and close by the horizon and
 * wider and taller nearer, as a sea recedes; each row glides along, the nearer ones faster, and rises
 * and falls on a swell that rolls down the rows, and foam laps along the horizon. Every pixel is a
 * flat token colour on one pixel grid, so nothing blends. It is decoration, hidden from assistive
 * technology, and still with motion reduced.
 */

type Shape = readonly string[];

/**
 * One row of waves, far to near: a crest every `length` cells, `height` cells from trough to crest,
 * `gap` cells of open water under it, gliding `speed` cells a second.
 */
type Wave = { length: number; height: number; gap: number; speed: number };

/** The rows from the horizon in; the last repeats down to the bottom of the sea. */
export const WAVES: readonly Wave[] = [
  { length: 8, height: 1, gap: 2, speed: 0.4 },
  { length: 10, height: 1, gap: 2, speed: 0.5 },
  { length: 13, height: 2, gap: 3, speed: 0.65 },
  { length: 16, height: 2, gap: 3, speed: 0.8 },
  { length: 20, height: 3, gap: 4, speed: 1 },
  { length: 25, height: 3, gap: 5, speed: 1.2 },
  { length: 31, height: 4, gap: 6, speed: 1.4 },
  { length: 38, height: 5, gap: 8, speed: 1.6 },
];

/** Deep enough for the tallest sea the opening draws, in cells; rows past its bottom are clipped. */
export const SEA_DEPTH = 180;

/** Where the crest stands along each wave: before the middle, so the face in front, toward the left
 *  where the rows glide, is steeper than the back. */
const CREST_AT = 0.38;

/** How sharply each wave peaks: the higher, the flatter its trough and the sharper its crest. */
const SHARPNESS = 2.4;

/**
 * How high a wave stands at `x` of `length`, 0 in the trough to 1 at the crest: flat in the trough
 * and sharp at the crest, as a real wave is, its face steeper than its back.
 */
function rise(x: number, length: number): number {
  const u = (((x + 0.5) / length) % 1 + 1) % 1;
  const t = u < CREST_AT ? u / CREST_AT : (1 - u) / (1 - CREST_AT);
  return t ** SHARPNESS;
}

/** The low part of each trough, as a share of the way up to the crest, left as open water so each
 *  wave is its own stroke rather than one line across the sea. */
const OPEN_WATER = 0.3;

/** Above this rise, a wave two or more cells high breaks white. */
const WHITECAP = 0.8;

/**
 * One wave as a tile: an unbroken stroke of `c` cells up its face, over its crest and down its back,
 * open water in the trough, and `w` cells of white water where a wave tall enough breaks.
 */
export function waveTile({ length, height }: Wave): Shape {
  const rows = Array.from({ length: height + 1 }, () => Array.from({ length }, () => "."));
  const top = (x: number) => Math.round(height * (1 - rise(x, length)));
  const drawn = (x: number) => rise(x, length) >= OPEN_WATER ** SHARPNESS;
  for (let x = 0; x < length; x++) {
    if (!drawn(x)) continue;
    const y = top(x);
    const before = drawn(x - 1) ? top(x - 1) : y;
    for (let d = Math.min(y, before + 1); d <= Math.max(y, before - 1); d++) rows[d]![x] = "c";
    if (height >= 2 && rise(x, length) >= WHITECAP) rows[y]![x] = "w";
  }
  return rows.map((r) => r.join(""));
}

/** Every row the sea draws, far to near, with where it starts in cells below the horizon. */
export function swell(depth = SEA_DEPTH): { wave: Wave; from: number; shift: number }[] {
  const out: { wave: Wave; from: number; shift: number }[] = [];
  for (let from = 1, i = 0; from < depth; i++) {
    const wave = WAVES[Math.min(i, WAVES.length - 1)]!;
    out.push({ wave, from, shift: (i * 7) % wave.length });
    from += wave.height + 1 + wave.gap;
  }
  return out;
}

/** Spray along the horizon, which laps a cell back and forth. */
export const FOAM_TILE: Shape = ["ccccc..c.c.."];

/** A shape's filled cells as runs along each row, `cell` pixels a cell. */
export function runs(shape: Shape, cell: number): { x: number; y: number; w: number }[] {
  const out: { x: number; y: number; w: number }[] = [];
  shape.forEach((row, y) => {
    for (const m of row.matchAll(/[^.]+/g)) out.push({ x: (m.index ?? 0) * cell, y: y * cell, w: m[0].length * cell });
  });
  return out;
}

function Cells({ shape, cell, fill, x = 0, y = 0 }: { shape: Shape; cell: number; fill: string; x?: number; y?: number }) {
  return runs(shape, cell).map((r) => <rect key={`${r.x},${r.y}`} x={x + r.x} y={y + r.y} width={r.w} height={cell} fill={fill} />);
}

/** One row of waves, as wide as the sea and one wave more, so its glide never shows an end. */
function Row({ row, index, cell }: { row: ReturnType<typeof swell>[number]; index: number; cell: number }) {
  const { wave, from, shift } = row;
  const id = `sea-wave-${index}-${cell}`;
  const tile = wave.length * cell;
  const shape = waveTile(wave);
  const heave = Math.max(1, Math.round((cell * wave.height) / 3));
  const motion = {
    "--tile": `${tile}px`,
    "--tile-px": tile,
    "--tile-dpx": tile * 2,
    "--glide": `${(wave.length / wave.speed).toFixed(2)}s`,
    "--heave": `${heave}px`,
    "--heave-px": heave,
    "--heave-dpx": heave * 2,
    "--beat": `-${(index * 0.42).toFixed(2)}s`,
  };
  return (
    <div
      className={`absolute left-0 ${styles.wave}`}
      style={{ top: from * cell, height: shape.length * cell, width: `calc(100% + ${tile}px)`, ...motion } as CSSProperties}
      data-slot="sea-wave"
    >
      <svg width="100%" height="100%" shapeRendering="crispEdges" className="block">
        <defs>
          <pattern id={id} x={-shift * cell} width={tile} height={shape.length * cell} patternUnits="userSpaceOnUse">
            <Cells shape={shape.map((r) => r.replace(/w/g, "c"))} cell={cell} fill="var(--tide-muted)" />
            <Cells shape={shape.map((r) => r.replace(/c/g, "."))} cell={cell} fill="var(--tide-foreground)" />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill={`url(#${id})`} />
      </svg>
    </div>
  );
}

/** The sea at one size of cell; `className` shows it only at the widths that use that size. */
function Sea({ cell, className }: { cell: 6 | 8; className: string }) {
  const foam = `sea-foam-${cell}`;
  return (
    <div className={className} style={{ "--cell": `${cell}px`, "--cell-px": cell, "--cell-dpx": cell * 2 } as CSSProperties}>
      <div className="absolute inset-0 overflow-hidden">
        <svg width="100%" height="100%" shapeRendering="crispEdges" className="absolute inset-0">
          <rect width="100%" height="100%" fill="var(--tide)" />
        </svg>
        {swell().map((row, i) => (
          <Row key={i} row={row} index={i} cell={cell} />
        ))}
        <svg height={cell} shapeRendering="crispEdges" className={`absolute top-0 ${styles.foam}`}>
          <defs>
            <pattern id={foam} width={FOAM_TILE[0]!.length * cell} height={cell} patternUnits="userSpaceOnUse">
              <Cells shape={FOAM_TILE} cell={cell} fill="var(--tide-muted)" />
            </pattern>
          </defs>
          <rect width="100%" height={cell} fill={`url(#${foam})`} />
        </svg>
      </div>
    </div>
  );
}

export function PixelSea({ className }: { className?: string }) {
  return (
    <div aria-hidden className={`${styles.sea} pointer-events-none ${className ?? ""}`} data-slot="pixel-sea" data-ambient="">
      <Sea cell={6} className="absolute inset-0 sm:hidden" />
      <Sea cell={8} className="absolute inset-0 max-sm:hidden" />
    </div>
  );
}
