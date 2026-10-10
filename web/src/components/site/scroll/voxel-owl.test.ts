import { describe, expect, it, vi } from "vitest";
import { OWL_HEIGHT, OWL_ROWS, type OwlInks, type Pose, TONES, type Voxel, drawOwl, halfDepth, owlVoxels, partOf, shade, skin } from "./voxel-owl";

/**
 * The owl in three dimensions (DEC-907): seen from the front it is its sprite pixel for pixel, an
 * owl rather than a cat (a round head with no ears, a facial disc, big eyes and a hooked beak), as
 * deep on the left as on the right, its head and wings move on their own without opening a hole, and
 * it paints only flat tones.
 */

const INKS: OwlInks = {
  line: "oklch(0.2 0.01 255)",
  feathers: "oklch(0.2 0.01 255)",
  trim: "oklch(0.38 0.063 192)",
  disc: "oklch(0.97 0.01 255)",
  iris: "oklch(0.88 0.149 90)",
  pupil: "oklch(0.175 0.006 255)",
  glint: "oklch(0.992 0.003 255)",
  beak: "oklch(0.2 0.01 255)",
};
const FRONT: Pose = { yaw: 0, pitch: 0, roll: 0, head: 0, flap: 0, stretch: 1, blink: false };
const MID_X = 7.5;
const MID_Y = (OWL_HEIGHT - 1) / 2;

/** The cube nearest the viewer at each sprite pixel, keyed by column and row. */
function frontView(voxels: Voxel[]): Map<string, Voxel> {
  const front = new Map<string, Voxel>();
  for (const v of voxels) {
    const key = `${v.x + MID_X},${MID_Y - v.y}`;
    const seen = front.get(key);
    if (!seen || v.z > seen.z) front.set(key, v);
  }
  return front;
}

/** Every corner a drawing passes through, as the fake canvas sees it. */
function drawn(pose: Pose) {
  const xs: number[] = [];
  const ys: number[] = [];
  const g = { beginPath: vi.fn(), moveTo: (x: number, y: number) => (xs.push(x), ys.push(y)), lineTo: (x: number, y: number) => (xs.push(x), ys.push(y)), closePath: vi.fn(), fill: vi.fn(), stroke: vi.fn() };
  drawOwl(g as unknown as CanvasRenderingContext2D, skin(owlVoxels(pose.blink)), pose, INKS, 0, 0, 1);
  return { width: Math.max(...xs) - Math.min(...xs), top: Math.min(...ys), fills: g.fill.mock.calls.length };
}

describe("the voxel owl", () => {
  it("is its sprite seen from the front: every pixel and nothing else, each in its ink", () => {
    const front = frontView(owlVoxels());
    const expected: Record<string, string> = { o: "line", b: "feathers", d: "trim", r: "trim", s: "trim", l: "disc", c: "disc", y: "iris", p: "pupil", g: "glint", k: "beak", K: "beak", f: "beak" };
    let pixels = 0;
    OWL_ROWS.forEach((row, y) =>
      [...row].forEach((c, x) => {
        const v = front.get(`${x},${y}`);
        if (c === ".") return expect(v, `${x},${y}`).toBeUndefined();
        pixels++;
        expect(v?.ink, `${x},${y} is ${c}`).toBe(expected[c]);
      }),
    );
    expect(front.size).toBe(pixels);
  });

  it("is drawn as an owl, not a cat: a round head with no ears, a disc ringed round two big eyes, a beak between them, taller than wide", () => {
    expect(OWL_ROWS.every((row) => row.length === 16)).toBe(true);
    expect(OWL_HEIGHT).toBeGreaterThan(16);
    const [top, second] = OWL_ROWS;
    expect(top!.indexOf("o"), "the head's top is a dome, not two ear points").toBeGreaterThan(2);
    expect(top!.match(/o+/g), "one run of outline across the crown").toHaveLength(1);
    expect(second!.match(/o+/g)).toHaveLength(2);
    const eyes = OWL_ROWS.join("").match(/[ypg]/g) ?? [];
    expect(eyes.length, "each eye is a 4 by 5 round").toBe(2 * 16);
    expect(OWL_ROWS.some((row) => /[ypg]kk[ypg]/.test(row)), "the beak sits between the eyes").toBe(true);
    const rim = OWL_ROWS.findIndex((row) => row.includes("r"));
    const lastRim = OWL_ROWS.findLastIndex((row) => row.includes("r"));
    const eyeRows = OWL_ROWS.map((row, i) => (/[ypg]/.test(row) ? i : -1)).filter((i) => i >= 0);
    expect(rim, "the disc's rim closes above the eyes").toBeLessThan(Math.min(...eyeRows));
    expect(lastRim, "and below them").toBeGreaterThan(Math.max(...eyeRows));
  });

  it("has two open eyes, each a 2 by 2 pupil with a glint, and shuts them in a blink", () => {
    const of = (voxels: Voxel[], ink: string) => [...frontView(voxels).values()].filter((v) => v.ink === ink).length;
    expect(of(owlVoxels(false), "pupil")).toBe(6);
    expect(of(owlVoxels(false), "glint")).toBe(2);
    const shut = owlVoxels(true);
    expect(of(shut, "iris") + of(shut, "pupil") + of(shut, "glint")).toBe(0);
  });

  it("is as deep on the left as on the right, deepest down the middle, and shallow at the crown", () => {
    for (let x = 0; x < 16; x++) expect(halfDepth(x, 10, "b"), `column ${x}`).toBe(halfDepth(15 - x, 10, "b"));
    expect(halfDepth(7, 10, "b")).toBeGreaterThan(halfDepth(1, 10, "b"));
    expect(halfDepth(7, 0, "b")).toBeLessThanOrEqual(3);
  });

  it("splits into a head, a body and two wings that mirror each other", () => {
    expect(partOf(7, 4)).toBe("head");
    expect(partOf(7, 15)).toBe("body");
    for (let y = 0; y < OWL_HEIGHT; y++) {
      for (let x = 0; x < 16; x++) {
        const part = partOf(x, y);
        const mirror = partOf(15 - x, y);
        expect(part === "wing-left" ? mirror === "wing-right" : part === "wing-right" ? mirror === "wing-left" : mirror === part, `${x},${y}`).toBe(true);
        if (part.startsWith("wing") && OWL_ROWS[y]![x] !== ".") expect("od", `${x},${y}`).toContain(OWL_ROWS[y]![x]);
      }
    }
  });

  it("keeps only the faces no other cube of the same part covers, so a lifted wing or a turned head never shows a hole", () => {
    const voxels = owlVoxels();
    const filled = new Set(voxels.map((v) => `${v.part}:${v.x},${v.y},${v.z}`));
    const steps = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] as const;
    const open = voxels.reduce((n, v) => n + steps.filter(([dx, dy, dz]) => !filled.has(`${v.part}:${v.x + dx},${v.y + dy},${v.z + dz}`)).length, 0);
    expect(skin(voxels).length).toBe(open);
  });

  it("spreads its wings wider and higher as they lift, and they fold back to where they were", () => {
    const folded = drawn(FRONT);
    const half = drawn({ ...FRONT, flap: 0.8 });
    const up = drawn({ ...FRONT, flap: 1.6 });
    expect(half.width).toBeGreaterThan(folded.width);
    expect(up.width).toBeGreaterThan(half.width);
    expect(drawn({ ...FRONT, flap: 0 })).toEqual(folded);
  });

  it("turns its head without moving its body: a turned head shows feathers where the disc was", () => {
    const facing = [...frontView(owlVoxels()).values()].filter((v) => v.part === "head" && v.ink === "disc").length;
    expect(facing).toBeGreaterThan(20);
    const turned = drawn({ ...FRONT, head: Math.PI });
    const straight = drawn(FRONT);
    expect(turned.top).toBeCloseTo(straight.top, 0);
    expect(turned.width).toBeCloseTo(straight.width, 0);
  });
});

describe("its shading", () => {
  it("scales only an oklch colour's lightness, and keeps any other colour as it is", () => {
    expect(shade("oklch(0.5 0.1 192)", 1)).toBe("oklch(0.5 0.1 192)");
    expect(shade("oklch(0.5 0.1 192)", 3)).toBe(`oklch(${0.5 * TONES[3]} 0.1 192)`);
    expect(shade("oklch(50% 0.1 192 / 0.5)", 0)).toBe(`oklch(${0.5 * TONES[0]} 0.1 192 / 0.5)`);
    expect(shade("oklch(0.98 0.01 90)", 0)).toBe("oklch(0.99 0.01 90)");
    expect(shade("transparent", 2)).toBe("transparent");
  });

  it("paints every face in one of four flat tones of its ink, and nothing turned away", () => {
    const fills = new Set<string>();
    const g = { beginPath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), closePath: vi.fn(), fill: vi.fn(), stroke: vi.fn(), lineJoin: "", lineWidth: 0, strokeStyle: "", set fillStyle(v: string) { fills.add(v); } };
    const faces = skin(owlVoxels());
    drawOwl(g as unknown as CanvasRenderingContext2D, faces, { ...FRONT, yaw: 0.6, pitch: 0.2, head: -0.4, flap: 0.9 }, INKS, 80, 80, 5);
    const allowed = new Set(Object.values(INKS).flatMap((c) => [0, 1, 2, 3].map((t) => shade(c, t))));
    for (const f of fills) expect(allowed, f).toContain(f);
    expect(g.fill.mock.calls.length).toBeLessThan(faces.length);
    expect(g.fill.mock.calls.length).toBeGreaterThan(faces.length / 4);
  });
});
