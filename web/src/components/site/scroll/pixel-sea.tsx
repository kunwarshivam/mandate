import type { CSSProperties, ReactNode } from "react";
import styles from "./scroll.module.css";

/**
 * The opening's sea (DEC-908): the long page starts on Hiroshige's bay in pixels, as the request for
 * a place ends on his full moon over it. A tide sea runs behind the agent's page from just above it
 * to below it; a moon in sun rises out of it as the opening comes into view, its light glinting on
 * the water, gulls flap and glide over it, and two pine islands stand on the horizon. The water is
 * rows of curling crests, small and close by the horizon and wider and taller nearer, as a sea
 * recedes; each row glides along, the nearer ones faster, and rises and falls on a swell that rolls
 * down the rows, and foam laps along the horizon. Every pixel is a flat token colour on the thread's
 * grid, so nothing blends. It is decoration, hidden from assistive technology, and still, half
 * risen, with motion reduced.
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

export const LEFT_ISLAND: Shape = [
  "............x.......",
  "...........xxx......",
  "..........xxxxx.....",
  ".........xxxxxxx....",
  "............x.......",
  ".......xxxxxxxxxx...",
  "...xxxxxxxxxxxxxxxx.",
  "xxxxxxxxxxxxxxxxxxxx",
];

export const RIGHT_ISLAND: Shape = [
  "....xx......",
  "..xxxxxx....",
  ".xxxxxxxxx..",
  "xxxxxxxxxxxx",
];

/** A gull with its wings up, and with them down: the two frames of a beat. */
export const GULL_UP: Shape = ["xx...xx", "..x.x..", "...x..."];
export const GULL_DOWN: Shape = ["...x...", "..x.x..", "xx...xx"];

/** Where each gull flies in the flock, in cells, as the geese cross the moon in the print. */
export const FLOCK = [
  { x: 0, y: 0 },
  { x: 10, y: 2 },
  { x: 19, y: 4 },
] as const;

export const MOON_RADIUS = 7;

/** The moon's cells, a disc `MOON_RADIUS` cells across each way from its centre. */
export function moonShape(r = MOON_RADIUS): Shape {
  const size = r * 2;
  return Array.from({ length: size }, (_, y) =>
    Array.from({ length: size }, (_, x) => ((x + 0.5 - r) ** 2 + (y + 0.5 - r) ** 2 <= r * r ? "x" : ".")).join(""),
  );
}

/** The moon's light on the water under it: broken bars, every other row, narrowing as they near. */
export const REFLECTION: Shape = [
  "..............",
  "..xxxxxxxxx...",
  "..............",
  "....xxxxxxxx..",
  "..............",
  "...xxxxxx.....",
  "..............",
  ".....xxxxx....",
  "..............",
  "....xxx.......",
  "..............",
  "......xx......",
];

/** The same light a moment later, each bar slipped a cell or broken, so it glints. */
export const GLINT: Shape = [
  "..............",
  "...xxxxxxxxx..",
  "..............",
  "...xxxxx.xx...",
  "..............",
  "....xxxxxx....",
  "..............",
  "....xxxxx.....",
  "..............",
  ".....xxx......",
  "..............",
  ".....xx.......",
];

const MOON = moonShape();

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

/** A small picture sized to `shape`'s cells; `children` draws in it, or the shape itself. */
function Sprite({ shape, cell, fill, className, slot, children }: { shape: Shape; cell: number; fill: string; className?: string; slot?: string; children?: ReactNode }) {
  return (
    <svg width={shape[0]!.length * cell} height={shape.length * cell} shapeRendering="crispEdges" className={className} overflow="visible" data-slot={slot}>
      {children ?? <Cells shape={shape} cell={cell} fill={fill} />}
    </svg>
  );
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

/** Where the moon and its reflection stand across the sea. */
const MOON_AT = "left-[26%] sm:left-[12%]";

const flockWidth = Math.max(...FLOCK.map((g) => g.x)) + GULL_UP[0]!.length;
const flockHeight = Math.max(...FLOCK.map((g) => g.y)) + GULL_UP.length;
const FLOCK_BOX: Shape = Array.from({ length: flockHeight }, () => ".".repeat(flockWidth));

/** The sea at one size of cell; `className` shows it only at the widths that use that size. */
function Sea({ cell, className }: { cell: 6 | 8; className: string }) {
  const foam = `sea-foam-${cell}`;
  return (
    <div className={className} style={{ "--cell": `${cell}px`, "--cell-px": cell, "--cell-dpx": cell * 2, "--rise-px": MOON_RADIUS * cell, "--rise-dpx": MOON_RADIUS * cell * 2 } as CSSProperties}>
      <Sprite shape={MOON} cell={cell} fill="var(--highlight)" className={`absolute bottom-full ${MOON_AT} ${styles.moon}`} slot="sea-moon" />
      <Sprite shape={FLOCK_BOX} cell={cell} fill="var(--sea-land)" className="absolute bottom-full left-[15%] mb-24 max-lg:hidden">
        {FLOCK.map((g, i) => (
          <g key={i} style={{ "--i": i } as CSSProperties}>
            <g className={styles.wingsUp}>
              <Cells shape={GULL_UP} cell={cell} fill="var(--sea-land)" x={g.x * cell} y={g.y * cell} />
            </g>
            <g className={styles.wingsDown} data-slot="gull-down">
              <Cells shape={GULL_DOWN} cell={cell} fill="var(--sea-land)" x={g.x * cell} y={g.y * cell} />
            </g>
          </g>
        ))}
      </Sprite>
      <Sprite shape={LEFT_ISLAND} cell={cell} fill="var(--sea-land)" className="absolute bottom-full left-[2%]" />
      <Sprite shape={RIGHT_ISLAND} cell={cell} fill="var(--sea-land)" className="absolute right-[2%] bottom-full" />
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
      <Sprite shape={REFLECTION} cell={cell} fill="var(--highlight)" className={`absolute top-0 ${MOON_AT}`}>
        <g className={styles.glintA}>
          <Cells shape={REFLECTION} cell={cell} fill="var(--highlight)" />
        </g>
        <g className={styles.glintB} data-slot="glint">
          <Cells shape={GLINT} cell={cell} fill="var(--highlight)" />
        </g>
      </Sprite>
    </div>
  );
}

export function PixelSea({ className }: { className?: string }) {
  return (
    <div aria-hidden className={`${styles.sea} pointer-events-none ${className ?? ""}`} data-slot="pixel-sea" data-thread-band="">
      <Sea cell={6} className="absolute inset-0 sm:hidden" />
      <Sea cell={8} className="absolute inset-0 max-sm:hidden" />
    </div>
  );
}
