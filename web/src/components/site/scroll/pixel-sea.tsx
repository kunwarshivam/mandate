import type { CSSProperties } from "react";
import styles from "./scroll.module.css";

/**
 * The opening's sea (DEC-908): the long page starts on Hiroshige's bay in pixels, as the request for
 * a place ends on his full moon over it. A tide sea runs behind the agent's page from just above it
 * to below it, its crests drifting a cell at a time; a moon in sun rises out of it with its reflection
 * laid on the water, geese cross it, and two pine islands stand on the horizon. Every pixel is a
 * flat token colour on the thread's grid, so the thread's knot on it lines up; there is no blend
 * anywhere. It is decoration, hidden from assistive technology, and still with motion reduced.
 */

type Shape = readonly string[];

/** A few crests of three sizes, scattered; the tile repeats across and down the sea. */
export const CREST_TILE: Shape = [
  "................................................",
  "................................................",
  "....ccc.........................................",
  "...c...c.................................ccc....",
  "........................................c...c...",
  "......................ccc.......................",
  "................................................",
  "................................................",
  "..................................ccc...........",
  ".................................c...c..........",
  "................................c.....c.........",
  ".............ccc................................",
  "............c...c...............................",
  "................................................",
  "..ccc.....................................ccc...",
  "................................................",
].map((row) => row.padEnd(48, "."));

/** Spray along the horizon, repeated. */
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

/** Geese crossing in a loose line, as they cross the moon in the print. */
export const GEESE: Shape = [
  "xx...xx..................",
  "..x.x....................",
  "...x.......xx...xx.......",
  ".............x.x.........",
  "..............x....xx...xx",
  "......................x.x.",
  ".......................x..",
].map((row) => row.padEnd(26, "."));

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

const MOON = moonShape();

/** A shape's filled cells as runs along each row, `cell` pixels a cell. */
export function runs(shape: Shape, cell: number): { x: number; y: number; w: number }[] {
  const out: { x: number; y: number; w: number }[] = [];
  shape.forEach((row, y) => {
    for (const m of row.matchAll(/[^.]+/g)) out.push({ x: (m.index ?? 0) * cell, y: y * cell, w: m[0].length * cell });
  });
  return out;
}

function Cells({ shape, cell, fill }: { shape: Shape; cell: number; fill: string }) {
  return runs(shape, cell).map((r) => <rect key={`${r.x},${r.y}`} x={r.x} y={r.y} width={r.w} height={cell} fill={fill} />);
}

/** A small picture of one shape, sized to its cells. */
function Sprite({ shape, cell, fill, className }: { shape: Shape; cell: number; fill: string; className: string }) {
  return (
    <svg width={shape[0]!.length * cell} height={shape.length * cell} shapeRendering="crispEdges" className={className}>
      <Cells shape={shape} cell={cell} fill={fill} />
    </svg>
  );
}

/** Where the moon and its reflection stand across the sea. */
const MOON_AT = "left-[26%] sm:left-[12%]";

/** The sea at one size of cell; `className` shows it only at the widths that use that size. */
function Sea({ cell, className }: { cell: 6 | 8; className: string }) {
  const crest = `sea-crest-${cell}`;
  const foam = `sea-foam-${cell}`;
  const tile = CREST_TILE[0]!.length * cell;
  return (
    <div className={className}>
      <Sprite shape={MOON} cell={cell} fill="var(--highlight)" className={`absolute bottom-full translate-y-1/2 ${MOON_AT}`} />
      <Sprite shape={GEESE} cell={cell} fill="var(--sea-land)" className="absolute bottom-full left-[15%] mb-24 max-lg:hidden" />
      <Sprite shape={LEFT_ISLAND} cell={cell} fill="var(--sea-land)" className="absolute bottom-full left-[2%]" />
      <Sprite shape={RIGHT_ISLAND} cell={cell} fill="var(--sea-land)" className="absolute right-[2%] bottom-full" />
      <svg width="100%" height="100%" shapeRendering="crispEdges" className="absolute inset-0" style={{ "--tile": `${tile}px` } as CSSProperties}>
        <defs>
          <pattern id={crest} width={tile} height={CREST_TILE.length * cell} patternUnits="userSpaceOnUse" y={cell * 2}>
            <Cells shape={CREST_TILE} cell={cell} fill="var(--tide-muted)" />
          </pattern>
          <pattern id={foam} width={FOAM_TILE[0]!.length * cell} height={cell} patternUnits="userSpaceOnUse">
            <Cells shape={FOAM_TILE} cell={cell} fill="var(--tide-muted)" />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill="var(--tide)" />
        <rect width="200%" height="100%" fill={`url(#${crest})`} className={styles.crests} data-slot="sea-crests" />
        <rect width="100%" height={cell} fill={`url(#${foam})`} />
      </svg>
      <Sprite shape={REFLECTION} cell={cell} fill="var(--highlight)" className={`absolute top-0 ${MOON_AT}`} />
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
