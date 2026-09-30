import { describe, expect, it } from "vitest";
import { type Board, chord, minesLeft, neighbours, newBoard, reveal, toggleFlag } from "./mines";

/** A board with mines exactly where given, already in play. */
function rigged(w: number, h: number, at: number[]): Board {
  const b = newBoard({ w, h, mines: at.length });
  const cells = b.cells.map((c, i) => ({ ...c, mine: at.includes(i) }));
  return { ...b, phase: "playing", cells: cells.map((c, i) => ({ ...c, near: neighbours(b, i).filter((n) => cells[n].mine).length })) };
}

describe("Minesweeper", () => {
  it("lays its mines on the first click, never on or next to the square clicked", () => {
    for (let seed = 0; seed < 50; seed++) {
      let s = seed + 1;
      const rand = () => ((s = (s * 16807) % 2147483647) - 1) / 2147483646;
      const b = reveal(newBoard(), 40, rand);
      expect(b.cells.filter((c) => c.mine)).toHaveLength(10);
      for (const i of [40, ...neighbours(b, 40)]) expect(b.cells[i].mine).toBe(false);
      expect(b.cells[40].open).toBe(true);
      expect(["playing", "won"]).toContain(b.phase);
    }
  });

  it("opens an empty patch as far as it goes and stops at the numbers", () => {
    const b = reveal(rigged(4, 4, [15]), 0);
    expect(b.cells.filter((c) => c.open)).toHaveLength(15);
    expect(b.phase).toBe("won");
    expect(b.cells[15].flag).toBe(true);
  });

  it("loses on a mine and shows every mine", () => {
    const b = reveal(rigged(3, 3, [0, 8]), 8);
    expect(b.phase).toBe("lost");
    expect(b.hit).toBe(8);
    expect(b.cells[0].open && b.cells[8].open).toBe(true);
  });

  it("flags, counts what is left, and never opens a flagged square", () => {
    let b = rigged(3, 3, [0]);
    b = toggleFlag(b, 0);
    expect(minesLeft(b)).toBe(0);
    expect(reveal(b, 0)).toBe(b);
    expect(minesLeft(toggleFlag(b, 0))).toBe(1);
  });

  it("chords: an open number with its flags placed opens the rest of its neighbours", () => {
    let b = rigged(3, 3, [0]);
    b = reveal(b, 4);
    b = toggleFlag(b, 0);
    b = chord(b, 4);
    expect(b.phase).toBe("won");
  });
});
