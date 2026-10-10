import { EYE_ROW, owlRows } from "@/components/domain/owl-sprite";

/**
 * The brand owl in three dimensions (DEC-907): its 16 by 16 sprite pushed out into cubes, deepest
 * down the middle and shallow at the ears, so it reads as round. Drawn on a canvas with the faces
 * sorted back to front, each face one flat tone of its colour, so no colour on the owl ever blends.
 */

export type VoxelInk = "line" | "feathers" | "wing" | "belly" | "eye" | "pupil" | "beak";

export interface Voxel {
  x: number;
  y: number;
  z: number;
  ink: VoxelInk;
}

/** Radians: `yaw` turns the head, `pitch` tips it towards you, `roll` leans it. */
export interface Pose {
  yaw: number;
  pitch: number;
  roll: number;
  /** Stretch along the vertical, 1 at rest; the width takes the inverse so the owl keeps its volume. */
  stretch: number;
  blink: boolean;
}

/** One CSS colour per ink, as the page's tokens resolve. */
export type OwlInks = Record<VoxelInk, string>;

const SIZE = 16;
const EYE_COLS = [2, 10] as const;

function inEye(x: number, y: number): "pupil" | "glint" | null {
  for (const col of EYE_COLS) {
    if (y >= EYE_ROW + 1 && y <= EYE_ROW + 2 && x >= col + 1 && x <= col + 2) return x === col + 1 && y === EYE_ROW + 1 ? "glint" : "pupil";
  }
  return null;
}

/** How far the owl reaches front and back at a column: half its depth, in cubes. */
export function halfDepth(x: number, y: number, ink: string): number {
  if (ink === "f") return 1;
  const t = (x - (SIZE - 1) / 2) / (SIZE / 2);
  const round = Math.max(1, Math.round(1.5 + 3.5 * Math.sqrt(Math.max(0, 1 - t * t))));
  return y < 3 ? Math.min(round, 2) : round;
}

function frontInk(c: string, x: number, y: number, blink: boolean): VoxelInk {
  const eye = inEye(x, y);
  if (c === "w" || eye) {
    if (blink) return "feathers";
    return eye === "pupil" ? "pupil" : "eye";
  }
  switch (c) {
    case "o":
      return "line";
    case "d":
      return "wing";
    case "l":
      return "belly";
    case "k":
    case "K":
    case "f":
      return "beak";
    default:
      return "feathers";
  }
}

/**
 * The owl's cubes, centred on the origin with y up. The outline, eyes and belly are drawn on the
 * front face only; behind it the owl is feathers, its wings wrap round the sides, and the eyes and
 * beak stand one cube proud of the face.
 */
export function owlVoxels(blink = false): Voxel[] {
  const out: Voxel[] = [];
  owlRows("tufts").forEach((row, y) => {
    [...row].forEach((c, x) => {
      if (c === ".") return;
      const h = halfDepth(x, y, c);
      const cx = x - (SIZE - 1) / 2;
      const cy = (SIZE - 1) / 2 - y;
      for (let z = -h; z < h; z++) {
        const front = z === h - 1;
        const ink: VoxelInk = front ? frontInk(c, x, y, blink) : c === "d" ? "wing" : c === "f" || c === "k" || c === "K" ? "beak" : "feathers";
        out.push({ x: cx, y: cy, z: z + 0.5, ink });
      }
      const proud = c === "w" || c === "k" || c === "K" || inEye(x, y) !== null;
      if (proud) out.push({ x: cx, y: cy, z: h + 0.5, ink: frontInk(c, x, y, blink) });
    });
  });
  return out;
}

type Vec = readonly [number, number, number];

interface Face {
  normal: Vec;
  corners: readonly Vec[];
  ink: VoxelInk;
}

const SIDES: { n: Vec; q: readonly Vec[] }[] = [
  { n: [0, 0, 1], q: [[-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]] },
  { n: [0, 0, -1], q: [[1, -1, -1], [-1, -1, -1], [-1, 1, -1], [1, 1, -1]] },
  { n: [1, 0, 0], q: [[1, -1, 1], [1, -1, -1], [1, 1, -1], [1, 1, 1]] },
  { n: [-1, 0, 0], q: [[-1, -1, -1], [-1, -1, 1], [-1, 1, 1], [-1, 1, -1]] },
  { n: [0, 1, 0], q: [[-1, 1, 1], [1, 1, 1], [1, 1, -1], [-1, 1, -1]] },
  { n: [0, -1, 0], q: [[-1, -1, -1], [1, -1, -1], [1, -1, 1], [-1, -1, 1]] },
];

/** Only the faces no other cube covers: the owl's skin. */
export function skin(voxels: Voxel[]): Face[] {
  const filled = new Set(voxels.map((v) => `${v.x},${v.y},${v.z}`));
  const faces: Face[] = [];
  for (const v of voxels) {
    for (const side of SIDES) {
      if (filled.has(`${v.x + side.n[0]},${v.y + side.n[1]},${v.z + side.n[2]}`)) continue;
      faces.push({ normal: side.n, corners: side.q.map((q) => [v.x + q[0] / 2, v.y + q[1] / 2, v.z + q[2] / 2] as const), ink: v.ink });
    }
  }
  return faces;
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
    const n = rotate(f.normal, pose);
    if (n[2] <= 0) continue;
    let depth = 0;
    const pts = f.corners.map((corner) => {
      const [x, y, z] = rotate(corner, pose);
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
    g.moveTo(p.pts[0][0], p.pts[0][1]);
    for (let i = 1; i < p.pts.length; i++) g.lineTo(p.pts[i][0], p.pts[i][1]);
    g.closePath();
    g.fillStyle = p.fill;
    g.strokeStyle = p.fill;
    g.fill();
    g.stroke();
  }
}
