import { describe, expect, it, vi } from "vitest";
import { owlRows } from "@/components/domain/owl-sprite";
import { type OwlInks, type Pose, type Voxel, TONES, drawOwl, halfDepth, owlVoxels, shade, skin } from "./voxel-owl";

/**
 * The owl in three dimensions (DEC-907) is the brand owl's own sprite: seen from the front it is the
 * 2D owl pixel for pixel, it is as deep on the left as on the right, and it paints only flat tones.
 */

const SPRITE = owlRows("tufts");
const INKS: OwlInks = { line: "oklch(0.2 0.01 255)", feathers: "oklch(0.88 0.149 90)", wing: "oklch(0.38 0.063 192)", belly: "oklch(0.97 0.01 255)", eye: "oklch(0.97 0.01 255)", pupil: "oklch(0.2 0.01 255)", beak: "oklch(0.38 0.063 192)" };
const FRONT: Pose = { yaw: 0, pitch: 0, roll: 0, stretch: 1, blink: false };

/** The cube nearest the viewer at each sprite pixel, keyed by column and row. */
function frontView(voxels: Voxel[]): Map<string, Voxel> {
  const front = new Map<string, Voxel>();
  for (const v of voxels) {
    const key = `${v.x + 7.5},${7.5 - v.y}`;
    const seen = front.get(key);
    if (!seen || v.z > seen.z) front.set(key, v);
  }
  return front;
}

describe("the voxel owl", () => {
  it("is the sprite seen from the front: every pixel and nothing else, outline, wings, belly and beak in their places", () => {
    const front = frontView(owlVoxels());
    const expected: Record<string, string> = { o: "line", d: "wing", l: "belly", b: "feathers", k: "beak", K: "beak", f: "beak" };
    let pixels = 0;
    SPRITE.forEach((row, y) =>
      [...row].forEach((c, x) => {
        const v = front.get(`${x},${y}`);
        if (c === ".") return expect(v, `${x},${y}`).toBeUndefined();
        pixels++;
        if (c !== "w") expect(v?.ink, `${x},${y} is ${c}`).toBe(expected[c]);
        else expect(["eye", "pupil"], `${x},${y}`).toContain(v?.ink);
      }),
    );
    expect(front.size).toBe(pixels);
  });

  it("has two open eyes, each a 2 by 2 pupil with a glint, and shuts them in a blink", () => {
    const pupils = (voxels: Voxel[]) => [...frontView(voxels).values()].filter((v) => v.ink === "pupil").length;
    expect(pupils(owlVoxels(false))).toBe(6);
    const shut = [...frontView(owlVoxels(true)).values()];
    expect(shut.some((v) => v.ink === "eye" || v.ink === "pupil")).toBe(false);
  });

  it("is as deep on the left as on the right, deepest down the middle, and shallow at the ears", () => {
    for (let x = 0; x < 16; x++) expect(halfDepth(x, 8, "b"), `column ${x}`).toBe(halfDepth(15 - x, 8, "b"));
    expect(halfDepth(7, 8, "b")).toBeGreaterThan(halfDepth(1, 8, "b"));
    expect(halfDepth(7, 1, "b")).toBeLessThanOrEqual(2);
  });

  it("keeps only the faces no other cube covers", () => {
    const voxels = owlVoxels();
    const filled = new Set(voxels.map((v) => `${v.x},${v.y},${v.z}`));
    const steps = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] as const;
    const open = voxels.reduce((n, v) => n + steps.filter(([dx, dy, dz]) => !filled.has(`${v.x + dx},${v.y + dy},${v.z + dz}`)).length, 0);
    expect(skin(voxels).length).toBe(open);
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
    drawOwl(g as unknown as CanvasRenderingContext2D, faces, { ...FRONT, yaw: 0.6, pitch: 0.2 }, INKS, 80, 80, 5);
    const allowed = new Set(Object.values(INKS).flatMap((c) => [0, 1, 2, 3].map((t) => shade(c, t))));
    for (const f of fills) expect(allowed, f).toContain(f);
    expect(g.fill.mock.calls.length).toBeLessThan(faces.length);
    expect(g.fill.mock.calls.length).toBeGreaterThan(faces.length / 4);
  });
});
