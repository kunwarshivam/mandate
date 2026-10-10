/**
 * The brand owl in three dimensions (DEC-907), drawn from its own sprite rather than the agents' one:
 * a tall, round body, a heart-shaped facial disc ringed in tide, two big sun eyes, a hooked beak and
 * folded tide wings, so it reads as an owl from any side. The sprite is pushed out into cubes,
 * deepest down the middle; its head turns on its own and its wings lift from the shoulders. Drawn on
 * a canvas with the faces sorted back to front, each face one flat tone of its colour, so no colour
 * on the owl ever blends.
 */

export type VoxelInk = "line" | "feathers" | "trim" | "disc" | "iris" | "pupil" | "glint" | "beak";

/** The parts that move on their own: the head turns, each wing lifts from its shoulder. */
export type OwlPart = "head" | "body" | "wing-left" | "wing-right";

export interface Voxel {
  x: number;
  y: number;
  z: number;
  ink: VoxelInk;
  part: OwlPart;
}

/** Radians: `yaw` turns the owl, `pitch` tips it towards you, `roll` leans it. */
export interface Pose {
  yaw: number;
  pitch: number;
  roll: number;
  /** How far the head turns from the body, in radians; an owl's turns a long way. */
  head: number;
  /** How far each wing lifts from its folded place, in radians. */
  flap: number;
  /** Stretch along the vertical, 1 at rest; the width takes the inverse so the owl keeps its volume. */
  stretch: number;
  blink: boolean;
}

/** One CSS colour per ink, as the page's tokens resolve. */
export type OwlInks = Record<VoxelInk, string>;

/**
 * The legend: o outline, b feathers, d wing, r the disc's rim, l the disc, c the chest, s the
 * chest's marks, y an iris, p a pupil, g its glint, k the beak, K its hooked tip, f a talon.
 */
export const OWL_ROWS = [
  "....oooooooo....",
  "..oobbbbbbbboo..",
  ".obbbbbbbbbbbbo.",
  "obrrrrrbbrrrrrbo",
  "orllllllllllllro",
  "orllyyllllyyllro",
  "orlyyyyllyyyylro",
  "orlygpykkygpylro",
  "orlyppykkyppylro",
  "orllyylKKlyyllro",
  "obrllllllllllrbo",
  "obbrrllllllrrbbo",
  "odbbbrrrrrrbbbdo",
  "oddbbccccccbbddo",
  "oddbscscscscbddo",
  "oddbccccccccbddo",
  "oddbcscscscsbddo",
  ".oddbccccccbddo.",
  "..oddbbbbbbddo..",
  "...oooooooooo...",
  "....fff..fff....",
] as const;

const WIDTH = 16;
export const OWL_HEIGHT = OWL_ROWS.length;
/** The last row of the head; below it the body and the wings. */
const NECK = 12;
/** The row the closed eyelids are drawn on. */
const LID_ROW = 7;
const MID_X = (WIDTH - 1) / 2;
const MID_Y = (OWL_HEIGHT - 1) / 2;
/** Where each wing hinges: the inner edge of its top row, in the owl's own units. */
const SHOULDER_X = 5;
const SHOULDER_Y = MID_Y - NECK + 0.5;

/** Which part a sprite pixel moves with: wing feathers and the outline beside them are the wings. */
export function partOf(x: number, y: number): OwlPart {
  if (y >= NECK && y < OWL_HEIGHT - 2) {
    const row = OWL_ROWS[y]!;
    const first = row.indexOf("d");
    const last = row.lastIndexOf("d");
    if (first >= 0 && x <= first + 1 && x < MID_X && (row[x] === "d" || x < first)) return "wing-left";
    if (last >= 0 && x >= last - 1 && x > MID_X && (row[x] === "d" || x > last)) return "wing-right";
  }
  return y <= NECK ? "head" : "body";
}

/** How far the owl reaches front and back at a column: half its depth, in cubes. */
export function halfDepth(x: number, y: number, c: string): number {
  if (c === "f") return 1;
  const t = (x - MID_X) / (WIDTH / 2);
  const round = Math.max(1, Math.round(1 + 2.6 * Math.sqrt(Math.max(0, 1 - t * t))));
  return y < 2 ? Math.min(round, 3) : round;
}

function frontInk(c: string, y: number, blink: boolean): VoxelInk {
  switch (c) {
    case "o":
      return "line";
    case "d":
    case "r":
    case "s":
      return "trim";
    case "l":
    case "c":
      return "disc";
    case "y":
    case "p":
    case "g":
      if (blink) return y === LID_ROW ? "line" : "disc";
      return c === "y" ? "iris" : c === "p" ? "pupil" : "glint";
    case "k":
    case "K":
    case "f":
      return "beak";
    default:
      return "feathers";
  }
}

function backInk(c: string): VoxelInk {
  if (c === "d") return "trim";
  if (c === "k" || c === "K" || c === "f") return "beak";
  return "feathers";
}

const PROUD = new Set(["y", "p", "g", "k", "K"]);

/**
 * The owl's cubes, centred on the origin with y up. The disc, eyes and chest are drawn on the front
 * face only; behind it the owl is feathers, its wings wrap round the sides, and the eyes and beak
 * stand one cube proud of the face.
 */
export function owlVoxels(blink = false): Voxel[] {
  const out: Voxel[] = [];
  OWL_ROWS.forEach((row, y) => {
    [...row].forEach((c, x) => {
      if (c === ".") return;
      const h = halfDepth(x, y, c);
      const part = partOf(x, y);
      const cx = x - MID_X;
      const cy = MID_Y - y;
      for (let z = -h; z < h; z++) {
        out.push({ x: cx, y: cy, z: z + 0.5, ink: z === h - 1 ? frontInk(c, y, blink) : backInk(c), part });
      }
      if (PROUD.has(c)) out.push({ x: cx, y: cy, z: h + 0.5, ink: frontInk(c, y, blink), part });
    });
  });
  return out;
}

type Vec = readonly [number, number, number];

export interface Face {
  normal: Vec;
  corners: readonly Vec[];
  ink: VoxelInk;
  part: OwlPart;
}

const SIDES: { n: Vec; q: readonly Vec[] }[] = [
  { n: [0, 0, 1], q: [[-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]] },
  { n: [0, 0, -1], q: [[1, -1, -1], [-1, -1, -1], [-1, 1, -1], [1, 1, -1]] },
  { n: [1, 0, 0], q: [[1, -1, 1], [1, -1, -1], [1, 1, -1], [1, 1, 1]] },
  { n: [-1, 0, 0], q: [[-1, -1, -1], [-1, -1, 1], [-1, 1, 1], [-1, 1, -1]] },
  { n: [0, 1, 0], q: [[-1, 1, 1], [1, 1, 1], [1, 1, -1], [-1, 1, -1]] },
  { n: [0, -1, 0], q: [[-1, -1, -1], [1, -1, -1], [1, -1, 1], [-1, -1, 1]] },
];

/** Only the faces no other cube of the same part covers: a lifted wing or a turned head never shows a hole. */
export function skin(voxels: Voxel[]): Face[] {
  const filled = new Set(voxels.map((v) => `${v.part}:${v.x},${v.y},${v.z}`));
  const faces: Face[] = [];
  for (const v of voxels) {
    for (const side of SIDES) {
      if (filled.has(`${v.part}:${v.x + side.n[0]},${v.y + side.n[1]},${v.z + side.n[2]}`)) continue;
      faces.push({ normal: side.n, corners: side.q.map((q) => [v.x + q[0] / 2, v.y + q[1] / 2, v.z + q[2] / 2] as const), ink: v.ink, part: v.part });
    }
  }
  return faces;
}

function turnY([x, y, z]: Vec, a: number): Vec {
  const c = Math.cos(a);
  const s = Math.sin(a);
  return [x * c + z * s, y, -x * s + z * c];
}

function turnZ([x, y, z]: Vec, a: number, ox = 0, oy = 0): Vec {
  const c = Math.cos(a);
  const s = Math.sin(a);
  const dx = x - ox;
  const dy = y - oy;
  return [ox + dx * c - dy * s, oy + dx * s + dy * c, z];
}

/** Moves a point, or a normal with `origin` false, with its part: the head turns about the neck, a wing about its shoulder. */
function withPart(p: Vec, part: OwlPart, pose: Pose, origin: boolean): Vec {
  switch (part) {
    case "head":
      return turnY(p, pose.head);
    case "wing-left":
      return origin ? turnZ(p, -pose.flap, -SHOULDER_X, SHOULDER_Y) : turnZ(p, -pose.flap);
    case "wing-right":
      return origin ? turnZ(p, pose.flap, SHOULDER_X, SHOULDER_Y) : turnZ(p, pose.flap);
    case "body":
      return p;
    default: {
      const never: never = part;
      return never;
    }
  }
}

function rotate([x0, y0, z0]: Vec, { yaw, pitch, roll }: Pose): Vec {
  let c = Math.cos(yaw);
  let s = Math.sin(yaw);
  const x1 = x0 * c + z0 * s;
  const z1 = -x0 * s + z0 * c;
  c = Math.cos(pitch);
  s = Math.sin(pitch);
  const y2 = y0 * c - z1 * s;
  const z2 = y0 * s + z1 * c;
  c = Math.cos(roll);
  s = Math.sin(roll);
  return [x1 * c - y2 * s, x1 * s + y2 * c, z2];
}

/** The light comes from the upper left, in front; a face takes one of four tones by how squarely it meets it. */
const LIGHT: Vec = (() => {
  const v = [-0.45, 0.75, 0.55] as const;
  const m = Math.hypot(...v);
  return [v[0] / m, v[1] / m, v[2] / m] as const;
})();
export const TONES = [1.08, 1, 0.86, 0.72] as const;

export function toneOf(normal: Vec): number {
  const lit = Math.max(0, normal[0] * LIGHT[0] + normal[1] * LIGHT[1] + normal[2] * LIGHT[2]);
  return lit > 0.8 ? 0 : lit > 0.45 ? 1 : lit > 0.15 ? 2 : 3;
}

/** A colour at one of the four tones: an OKLCH colour has its lightness scaled; anything else is kept as it is. */
export function shade(colour: string, tone: number): string {
  const trimmed = colour.trim();
  return trimmed.replace(/^(oklch\(\s*)([\d.]+)(%?)(?=\s)/, (whole, head: string, l: string, percent: string) => {
    const scaled = (Number(l) / (percent ? 100 : 1)) * TONES[tone];
    return `${head}${Math.min(0.99, Math.round(scaled * 1000) / 1000)}`;
  });
}

/** Perspective: how many cubes away the eye is. */
const EYE_DISTANCE = 70;

/**
 * Draws the owl centred at (`cx`, `cy`) on the context, `unit` pixels a cube. Faces turned away are
 * skipped, the rest are painted far to near, each filled and stroked in its tone so no seam shows.
 */
export function drawOwl(g: CanvasRenderingContext2D, faces: Face[], pose: Pose, inks: OwlInks, cx: number, cy: number, unit: number): void {
  const sx = 1 / Math.sqrt(pose.stretch);
  const sy = pose.stretch;
  const visible: { pts: [number, number][]; depth: number; fill: string }[] = [];
  const tones = new Map<string, string>();
  for (const f of faces) {
    const n = rotate(withPart(f.normal, f.part, pose, false), pose);
    if (n[2] <= 0) continue;
    let depth = 0;
    const pts = f.corners.map((corner) => {
      const [x, y, z] = rotate(withPart(corner, f.part, pose, true), pose);
      depth += z;
      const k = EYE_DISTANCE / (EYE_DISTANCE - z);
      return [cx + x * k * unit * sx, cy - y * k * unit * sy] as [number, number];
    });
    const tone = toneOf(n);
    const key = `${f.ink}${tone}`;
    let fill = tones.get(key);
    if (!fill) {
      fill = shade(inks[f.ink], tone);
      tones.set(key, fill);
    }
    visible.push({ pts, depth, fill });
  }
  visible.sort((a, b) => a.depth - b.depth);
  g.lineJoin = "round";
  g.lineWidth = 0.75;
  for (const p of visible) {
    g.beginPath();
    g.moveTo(p.pts[0]![0], p.pts[0]![1]);
    for (let i = 1; i < p.pts.length; i++) g.lineTo(p.pts[i]![0], p.pts[i]![1]);
    g.closePath();
    g.fillStyle = p.fill;
    g.strokeStyle = p.fill;
    g.fill();
    g.stroke();
  }
}
