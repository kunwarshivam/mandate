"use client";

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { Owl } from "@/components/domain/owl";
import { cn } from "@/lib/utils";
import { MONO, PIXEL, RAISED, SUNKEN } from "./letter";
import { type Board, type Cell, chord, minesLeft, newBoard, reveal, toggleFlag } from "./mines";
import { MINE, PixelIcon } from "./pixel-icons";

const NUMBER = ["", "text-series-1", "text-gain", "text-loss", "text-mandate-strong", "text-loss", "text-series-3", "text-foreground", "text-muted-foreground"];

const LONG_PRESS = 450;

function Led({ value, label }: { value: number; label: string }) {
  const shown = value < 0 ? `-${String(Math.min(99, -value)).padStart(2, "0")}` : String(Math.min(999, value)).padStart(3, "0");
  return (
    <span role="img" aria-label={`${label}: ${value}`} className={cn(SUNKEN, MONO, "w-[3.25rem] bg-ink px-1 text-center text-[1.625rem] leading-none text-highlight tabular-nums")}>
      {shown}
    </span>
  );
}

function describe(c: Cell, lost: boolean): string {
  if (c.flag) return lost && !c.mine ? "wrong flag" : "flagged";
  if (!c.open) return "hidden";
  if (c.mine) return "mine";
  return c.near === 0 ? "empty" : String(c.near);
}

/** Minesweeper, beginner's board. The owl is the face: it watches while you play and closes its eyes when you lose. */
export function Minesweeper() {
  const [board, setBoard] = useState<Board>(newBoard);
  const [pressing, setPressing] = useState(false);
  const [focus, setFocus] = useState(40);
  const [seconds, setSeconds] = useState(0);
  const grid = useRef<HTMLDivElement>(null);
  const hold = useRef<{ timer: ReturnType<typeof setTimeout>; fired: boolean } | null>(null);
  const over = board.phase === "won" || board.phase === "lost";

  useEffect(() => {
    if (board.phase !== "playing") return;
    const t = setInterval(() => setSeconds((s) => s + 1), 1000);
    return () => clearInterval(t);
  }, [board.phase]);

  const open = (i: number) => setBoard(board.cells[i].open ? chord(board, i) : reveal(board, i));
  const flag = (i: number) => setBoard(toggleFlag(board, i));

  const restart = () => {
    setBoard(newBoard());
    setSeconds(0);
  };

  const move = (e: KeyboardEvent<HTMLDivElement>) => {
    const { w, h } = board;
    const x = focus % w;
    const y = Math.floor(focus / w);
    const to: Record<string, number> = { ArrowLeft: y * w + Math.max(0, x - 1), ArrowRight: y * w + Math.min(w - 1, x + 1), ArrowUp: Math.max(0, y - 1) * w + x, ArrowDown: Math.min(h - 1, y + 1) * w + x };
    if (e.key in to) {
      e.preventDefault();
      setFocus(to[e.key]);
      grid.current?.querySelectorAll<HTMLButtonElement>("button")[to[e.key]]?.focus();
    } else if (e.key === "f" || e.key === "F") {
      e.preventDefault();
      flag(focus);
    }
  };

  const face = board.phase === "lost" ? "stopped" : pressing ? "focused" : "awake";

  return (
    <div className={cn(PIXEL, "grid justify-center gap-2 overflow-auto p-2 sm:p-3")} onPointerUp={() => setPressing(false)} onPointerLeave={() => setPressing(false)}>
      <div className={cn(SUNKEN, "flex items-center justify-between gap-2 p-1.5")}>
        <Led value={minesLeft(board)} label="Mines left" />
        <button type="button" aria-label="New game" onClick={restart} className={cn(RAISED, "grid size-9 cursor-pointer place-items-center bg-muted outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:outline-foreground active:border-t-foreground/60 active:border-l-foreground/60 active:border-r-card active:border-b-card")}>
          <Owl seed="minesweeper" mood={face} className="size-7" />
        </button>
        <Led value={seconds} label="Seconds" />
      </div>

      <div
        ref={grid}
        role="grid"
        aria-label="Minefield"
        aria-rowcount={board.h}
        aria-colcount={board.w}
        onKeyDown={move}
        className={cn(SUNKEN, "grid w-fit")}
        style={{ gridTemplateColumns: `repeat(${board.w}, 1.625rem)` }}
        data-slot="minefield"
        data-phase={board.phase}
      >
        {board.cells.map((c, i) => {
          const wrong = board.phase === "lost" && c.flag && !c.mine;
          return (
            <button
              key={i}
              type="button"
              tabIndex={i === focus ? 0 : -1}
              aria-label={`Row ${Math.floor(i / board.w) + 1}, column ${(i % board.w) + 1}: ${describe(c, board.phase === "lost")}`}
              onFocus={() => setFocus(i)}
              onPointerDown={(e) => {
                if (e.button !== 0 || over) return;
                setPressing(true);
                if (e.pointerType === "touch" && !c.open) {
                  const entry = { fired: false, timer: setTimeout(() => ((entry.fired = true), flag(i)), LONG_PRESS) };
                  hold.current = entry;
                }
              }}
              onPointerUp={() => hold.current && clearTimeout(hold.current.timer)}
              onClick={() => {
                if (hold.current?.fired) {
                  hold.current = null;
                  return;
                }
                hold.current = null;
                open(i);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                flag(i);
              }}
              className={cn(
                "grid size-[1.625rem] cursor-pointer place-items-center text-[1rem] leading-none font-semibold outline-none select-none focus-visible:outline-1 focus-visible:-outline-offset-4 focus-visible:outline-dotted focus-visible:outline-foreground",
                c.open ? "border border-foreground/25 bg-muted" : cn(RAISED, "bg-muted"),
                c.open && !c.mine && NUMBER[c.near],
                board.hit === i && "bg-loss",
              )}
            >
              {c.open && c.mine ? <PixelIcon sprite={MINE} className="size-4" /> : c.flag ? <span className={cn(wrong ? "text-muted-foreground line-through" : "text-loss")}>⚑</span> : c.open && c.near > 0 ? c.near : null}
            </button>
          );
        })}
      </div>

      <p role="status" className="min-h-5 text-center text-[0.875rem]">
        {board.phase === "won" ? `Cleared in ${seconds} seconds. The owl approves.` : board.phase === "lost" ? "Boom. Press the owl to play again." : "Right-click or press F to flag. Hold to flag on a phone."}
      </p>
    </div>
  );
}
