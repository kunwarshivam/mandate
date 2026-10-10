import { cn } from "@/lib/utils";

/**
 * "Owlhead" as a bitmap logotype, the way a 1990s homepage set its name in a hand-drawn GIF: nine
 * pixels tall, two-pixel stems, in sentence case, with a dithered extrusion down and to the right,
 * the 50% checker a 1-bit screen drew grey with. Ink and checker only, flat fills as `web/DESIGN.md`
 * asks, so it is the same mark in both themes.
 */
export const GLYPHS: ReadonlyArray<{ char: string; rows: readonly string[] }> = [
  { char: "O", rows: [".######.", "###..###", "##....##", "##....##", "##....##", "##....##", "##....##", "###..###", ".######."] },
  { char: "w", rows: [".........", ".........", ".........", "##.....##", "##.....##", "##..#..##", "##.###.##", "#########", ".##...##."] },
  { char: "l", rows: ["###", ".##", ".##", ".##", ".##", ".##", ".##", ".##", ".##"] },
  { char: "h", rows: ["##.....", "##.....", "##.....", "######.", "###..##", "##...##", "##...##", "##...##", "##...##"] },
  { char: "e", rows: [".......", ".......", ".......", ".#####.", "##...##", "#######", "##.....", "##...##", ".#####."] },
  { char: "a", rows: [".......", ".......", ".......", ".#####.", ".....##", ".######", "##...##", "##...##", ".######"] },
  { char: "d", rows: [".....##", ".....##", ".....##", ".######", "##..###", "##...##", "##...##", "##...##", ".######"] },
];

export const WORDMARK_TEXT = GLYPHS.map((g) => g.char).join("");

const HEIGHT = 9;

/** One sprite pixel is this many units, so the dither can be half a sprite pixel and stay crisp. */
const UNIT = 2;

/** How far the extrusion reaches, in units. */
const DEPTH = 3;

/** The lit sprite pixels, in units, as `x,y` keys. */
function face(): Set<string> {
  const on = new Set<string>();
  let left = 0;
  for (const { rows } of GLYPHS) {
    rows.forEach((row, y) => {
      [...row].forEach((c, x) => {
        if (c !== "#") return;
        for (let dy = 0; dy < UNIT; dy++) for (let dx = 0; dx < UNIT; dx++) on.add(`${(left + x) * UNIT + dx},${y * UNIT + dy}`);
      });
    });
    left += rows[0].length + 1;
  }
  return on;
}

export const COLUMNS = GLYPHS.reduce((w, g) => w + g.rows[0].length, 0) + GLYPHS.length - 1;

const WIDTH = COLUMNS * UNIT + DEPTH;

const TALL = HEIGHT * UNIT + DEPTH;

/** Cells as one path of horizontal runs, so the whole mark is two elements. */
function runs(cells: Set<string>): string {
  let d = "";
  for (let y = 0; y < TALL; y++) {
    for (let x = 0; x < WIDTH; x++) {
      if (!cells.has(`${x},${y}`)) continue;
      let w = 1;
      while (cells.has(`${x + w},${y}`)) w++;
      d += `M${x} ${y}h${w}v1h-${w}z`;
      x += w;
    }
  }
  return d;
}

const FACE = face();

const EXTRUSION = new Set(
  [...FACE].flatMap((k) => {
    const [x, y] = k.split(",").map(Number);
    return Array.from({ length: DEPTH }, (_, i) => `${x + i + 1},${y + i + 1}`);
  }).filter((k) => !FACE.has(k)),
);

const FACE_PATH = runs(FACE);

const EXTRUSION_PATH = runs(EXTRUSION);

export const VIEWBOX = `0 0 ${WIDTH} ${TALL}`;

/**
 * Drawn at whole multiples of a unit (2, 3 and 4 px), so every sprite pixel and every dither dot
 * lands on screen pixels.
 */
export function Wordmark({ className }: { className?: string }) {
  return (
    <svg
      aria-hidden
      viewBox={VIEWBOX}
      shapeRendering="crispEdges"
      className={cn("block h-auto [--u:2px] min-[25rem]:[--u:3px] md:[--u:4px]", className)}
      style={{ width: `calc(var(--u) * ${WIDTH})` }}
      data-slot="wordmark"
    >
      <defs>
        <pattern id="wordmark-dither" width="2" height="2" patternUnits="userSpaceOnUse">
          <rect width="1" height="1" className="fill-foreground" />
          <rect x="1" y="1" width="1" height="1" className="fill-foreground" />
        </pattern>
      </defs>
      <path d={EXTRUSION_PATH} fill="url(#wordmark-dither)" />
      <path d={FACE_PATH} className="fill-foreground" />
    </svg>
  );
}
