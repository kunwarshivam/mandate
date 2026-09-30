/**
 * Minesweeper's rules, apart from its window. Mines are laid on the first click, never on or next to
 * the square clicked, so the first click always opens a patch of board.
 */
export type Cell = { mine: boolean; open: boolean; flag: boolean; near: number };

export type Phase = "ready" | "playing" | "won" | "lost";

export type Board = { w: number; h: number; mines: number; cells: Cell[]; phase: Phase; hit: number | null };

export const BEGINNER = { w: 9, h: 9, mines: 10 } as const;

const EMPTY: Cell = { mine: false, open: false, flag: false, near: 0 };

export function newBoard({ w, h, mines }: { w: number; h: number; mines: number } = BEGINNER): Board {
  return { w, h, mines, cells: Array.from({ length: w * h }, () => ({ ...EMPTY })), phase: "ready", hit: null };
}

export function neighbours(b: Pick<Board, "w" | "h">, i: number): number[] {
  const x = i % b.w;
  const y = Math.floor(i / b.w);
  const out: number[] = [];
  for (let dy = -1; dy <= 1; dy++)
    for (let dx = -1; dx <= 1; dx++) {
      const nx = x + dx;
      const ny = y + dy;
      if ((dx || dy) && nx >= 0 && ny >= 0 && nx < b.w && ny < b.h) out.push(ny * b.w + nx);
    }
  return out;
}

function lay(b: Board, first: number, rand: () => number): Cell[] {
  const keep = new Set([first, ...neighbours(b, first)]);
  const spots = b.cells.map((_, i) => i).filter((i) => !keep.has(i));
  for (let i = spots.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [spots[i], spots[j]] = [spots[j], spots[i]];
  }
  const mined = new Set(spots.slice(0, b.mines));
  const cells = b.cells.map((c, i) => ({ ...c, mine: mined.has(i) }));
  return cells.map((c, i) => ({ ...c, near: neighbours(b, i).filter((n) => cells[n].mine).length }));
}

/** Opens a square. A square with no mines around it opens its neighbours too, as far as that goes. */
export function reveal(b: Board, i: number, rand: () => number = Math.random): Board {
  if (b.phase === "won" || b.phase === "lost" || b.cells[i].open || b.cells[i].flag) return b;
  const cells = b.phase === "ready" ? lay(b, i, rand) : b.cells.map((c) => ({ ...c }));
  if (cells[i].mine) {
    for (const c of cells) if (c.mine && !c.flag) c.open = true;
    return { ...b, cells, phase: "lost", hit: i };
  }
  const queue = [i];
  while (queue.length) {
    const at = queue.pop()!;
    const c = cells[at];
    if (c.open || c.flag) continue;
    c.open = true;
    if (c.near === 0) queue.push(...neighbours(b, at).filter((n) => !cells[n].open));
  }
  const won = cells.every((c) => c.mine || c.open);
  if (won) for (const c of cells) if (c.mine) c.flag = true;
  return { ...b, cells, phase: won ? "won" : "playing" };
}

/** Clicking an open number whose flags are all placed opens the rest of its neighbours. */
export function chord(b: Board, i: number): Board {
  const c = b.cells[i];
  if (b.phase !== "playing" || !c.open || c.near === 0) return b;
  const around = neighbours(b, i);
  if (around.filter((n) => b.cells[n].flag).length !== c.near) return b;
  return around.reduce((next, n) => reveal(next, n), b);
}

export function toggleFlag(b: Board, i: number): Board {
  if (b.phase === "won" || b.phase === "lost" || b.cells[i].open) return b;
  const cells = b.cells.map((c, j) => (j === i ? { ...c, flag: !c.flag } : c));
  return { ...b, cells };
}

export const minesLeft = (b: Board) => b.mines - b.cells.filter((c) => c.flag).length;
