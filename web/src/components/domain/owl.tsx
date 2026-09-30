"use client";

import { type CSSProperties, type RefObject, useEffect, useRef } from "react";
import { AnimatePresence, type MotionValue, motion, useReducedMotion, useSpring, useTransform } from "motion/react";
import type { Agent, AgentMode } from "@/fixtures/types";
import { cn } from "@/lib/utils";

/**
 * Each agent is a pixel owl (DEC-217): a 16 × 16 sprite whose ears, markings and feathers come
 * from its ID, so an agent keeps its face on every screen and across sessions. Its eyes say its
 * mode and nothing else: open when it trades, lidded on exits only, asleep when paused, shut when
 * stopped. Nothing about it follows P&L, so an owl never cheers a gain or sulks at a loss
 * (DESIGN.md, "No gamification"). Feathers take the series hues and never gain or loss.
 *
 * An awake owl blinks on its own rhythm and its pupils follow the pointer a whole pixel at a time;
 * a sleeping owl breathes; a stopped owl is still. Under reduced motion every owl is still.
 */
export type OwlMood = "awake" | "focused" | "asleep" | "stopped";

const MOOD: Record<AgentMode, OwlMood> = {
  normal: "awake",
  exits_only: "focused",
  paused: "asleep",
  stopped: "stopped",
};

export function moodFor(mode: AgentMode): OwlMood {
  return MOOD[mode];
}

/** FNV-1a: the same ID always draws the same owl. */
export function owlSeed(id: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

const FEATHERS = ["var(--series-1)", "var(--series-2)", "var(--series-3)", "var(--series-4)"] as const;
const EARS = ["tufts", "wide", "round"] as const;
const MARKING = ["none", "speckles", "bib"] as const;
const BLINK_EVERY = [3.8, 4.6, 5.4, 6.2] as const;

export interface OwlShape {
  feathers: (typeof FEATHERS)[number];
  ears: (typeof EARS)[number];
  marking: (typeof MARKING)[number];
  /** Seconds between blinks, and how far into that cycle this owl starts, so owls never blink together. */
  blink: { every: number; offset: number };
}

export function owlShape(id: string): OwlShape {
  const s = owlSeed(id);
  const pick = <T,>(list: readonly T[], shift: number) => list[(s >>> shift) % list.length];
  const every = pick(BLINK_EVERY, 20);
  return {
    feathers: pick(FEATHERS, 11),
    ears: pick(EARS, 3),
    marking: pick(MARKING, 14),
    blink: { every, offset: Math.round((((s >>> 23) % 100) / 100) * every * 10) / 10 },
  };
}

/** One sprite pixel, in viewBox units. */
const PX = 4;

const EAR_ROWS: Record<OwlShape["ears"], [string, string]> = {
  tufts: ["..b..........b..", "..bb........bb.."],
  wide: [".bb..........bb.", ".bbb........bbb."],
  round: ["................", "..bbb......bbb.."],
};

/** The head below the ears: b feathers, d wing shade, w eye white, k beak and feet. */
const HEAD = [
  "..bbbbbbbbbbbb..",
  ".bbbbbbbbbbbbbb.",
  ".bwwwwbbbbwwwwb.",
  "bbwwwwbbbbwwwwbb",
  "dbwwwwbbbbwwwwbd",
  "dbwwwwbkkbwwwwbd",
  "dbbbbbbkkbbbbbbd",
  ".dbbbbbbbbbbbbd.",
  ".bbbbbbbbbbbbbb.",
  "..bbbbbbbbbbbb..",
  "..bbbbbbbbbbbb..",
  "...bbbbbbbbbb...",
  "....kk....kk....",
  "................",
];

/** Markings over the chest: m a darker feather, e a pale one. */
const MARKING_ROWS: Record<OwlShape["marking"], Record<number, string>> = {
  none: {},
  speckles: { 10: "....m......m....", 11: ".......mm......." },
  bib: { 10: ".....eeeeee.....", 11: "......eeee......", 12: ".......ee......." },
};

/** The two eyes' whites start at these columns, on rows 4 to 7. */
const EYE_COLS = [2, 10] as const;
const EYE_ROW = 4;

/** Where the pupils may go: one pixel either way, snapped to the grid. */
const REACH = PX;
/** Past this distance from the owl, in CSS pixels, the pupils are at full reach. */
const FAR = 240;
const SPRING = { stiffness: 220, damping: 22, mass: 0.6 };

/** One listener for every owl on the page, fired at most once a frame. */
const watchers = new Set<(x: number, y: number) => void>();
let pending = 0;
let last: { x: number; y: number } | null = null;

function onPointer(e: PointerEvent) {
  last = { x: e.clientX, y: e.clientY };
  if (pending) return;
  pending = requestAnimationFrame(() => {
    pending = 0;
    if (last) for (const w of watchers) w(last.x, last.y);
  });
}

function watchPointer(watcher: (x: number, y: number) => void): () => void {
  if (watchers.size === 0) window.addEventListener("pointermove", onPointer, { passive: true });
  watchers.add(watcher);
  if (last) watcher(last.x, last.y);
  return () => {
    watchers.delete(watcher);
    if (watchers.size === 0) {
      window.removeEventListener("pointermove", onPointer);
      if (pending) cancelAnimationFrame(pending);
      pending = 0;
    }
  };
}

/**
 * Pupils that follow the pointer on a spring, `reach` viewBox units at most, and settle in the
 * middle when the pointer leaves or the eyes are shut. Reduced motion only gates the pointer here;
 * the stylesheet stops the rest, so the server's markup, which cannot know the preference, still
 * matches the client's.
 */
export function useGaze(ref: RefObject<SVGSVGElement | null>, { reach, looking }: { reach: number; looking: boolean }): [MotionValue<number>, MotionValue<number>] {
  const reduced = useReducedMotion() ?? false;
  const tracking = looking && !reduced;
  const gazeX = useSpring(0, SPRING);
  const gazeY = useSpring(0, SPRING);

  useEffect(() => {
    if (!tracking) {
      gazeX.jump(0);
      gazeY.jump(0);
      return;
    }
    const stop = watchPointer((x, y) => {
      const box = ref.current?.getBoundingClientRect();
      if (!box || box.width === 0) return;
      const dx = x - (box.left + box.width / 2);
      const dy = y - (box.top + box.height / 2);
      const distance = Math.hypot(dx, dy);
      if (distance < 1) return;
      const pull = Math.min(distance / FAR, 1) * reach;
      gazeX.set((dx / distance) * pull);
      gazeY.set((dy / distance) * pull);
    });
    const settle = () => {
      gazeX.set(0);
      gazeY.set(0);
    };
    document.documentElement.addEventListener("pointerleave", settle);
    return () => {
      stop();
      document.documentElement.removeEventListener("pointerleave", settle);
    };
  }, [tracking, reach, ref, gazeX, gazeY]);

  return [gazeX, gazeY];
}

/** Open eyes look about; closed ones stay put. */
export function looks(mood: OwlMood): boolean {
  switch (mood) {
    case "awake":
    case "focused":
      return true;
    case "asleep":
    case "stopped":
      return false;
    default: {
      const unhandled: never = mood;
      throw new Error(`unhandled owl mood ${String(unhandled)}`);
    }
  }
}

function Pixels({ rows, top, fill, opacity }: { rows: Record<number, string>; top: number; fill: (c: string) => string | null; opacity?: (c: string) => number | undefined }) {
  return Object.entries(rows).flatMap(([y, row]) =>
    [...row].flatMap((c, x) => {
      const f = fill(c);
      return f ? [<rect key={`${x}-${y}`} x={x * PX} y={(top + Number(y)) * PX} width={PX} height={PX} fill={f} opacity={opacity?.(c)} />] : [];
    }),
  );
}

/** Rows of pixels, `x` and `y` in sprite pixels. */
function block(x: number, y: number, w: number, h: number, fill: string, key: string) {
  return <rect key={key} x={x * PX} y={y * PX} width={w * PX} height={h * PX} fill={fill} />;
}

function Eyes({ mood, feathers, gaze }: { mood: OwlMood; feathers: string; gaze: [MotionValue<number>, MotionValue<number>] }) {
  const lid = (h: number) => EYE_COLS.map((x) => block(x, EYE_ROW, 4, h, feathers, `lid-${x}`));
  switch (mood) {
    case "awake":
    case "focused": {
      const dy = mood === "focused" ? 1 : 0;
      return (
        <>
          <motion.g style={{ x: gaze[0], y: gaze[1] }}>
            {EYE_COLS.map((x) => (
              <g key={x} className="owl-blink">
                {block(x + 1, EYE_ROW + 1 + dy, 2, 2, "var(--owl-pupil)", "pupil")}
              </g>
            ))}
          </motion.g>
          {mood === "focused" ? lid(2) : null}
        </>
      );
    }
    case "asleep":
      return (
        <>
          {lid(4)}
          {EYE_COLS.flatMap((x) => [block(x, EYE_ROW + 1, 1, 1, "var(--owl-pupil)", `a-${x}`), block(x + 3, EYE_ROW + 1, 1, 1, "var(--owl-pupil)", `b-${x}`), block(x + 1, EYE_ROW + 2, 2, 1, "var(--owl-pupil)", `c-${x}`)])}
        </>
      );
    case "stopped":
      return (
        <>
          {lid(4)}
          {EYE_COLS.map((x) => block(x, EYE_ROW + 2, 4, 1, "var(--owl-pupil)", `s-${x}`))}
        </>
      );
    default: {
      const unhandled: never = mood;
      throw new Error(`unhandled owl mood ${String(unhandled)}`);
    }
  }
}

/** A pixel "z", three pixels square, `p` viewBox units a pixel. */
function PixelZ({ x, y, p, className }: { x: number; y: number; p: number; className: string }) {
  return (
    <g className={className} fill="var(--muted-foreground)">
      <rect x={x} y={y} width={p * 3} height={p} />
      <rect x={x + p} y={y + p} width={p} height={p} />
      <rect x={x} y={y + p * 2} width={p * 3} height={p} />
    </g>
  );
}

export function Owl({
  seed,
  mood,
  still = false,
  className,
  style,
}: {
  seed: string;
  mood: OwlMood;
  /** Draw it without any motion, as in a static specimen. */
  still?: boolean;
  className?: string;
  style?: CSSProperties;
}) {
  const shape = owlShape(seed);
  const ref = useRef<SVGSVGElement>(null);
  const [gazeX, gazeY] = useGaze(ref, { reach: REACH, looking: !still && looks(mood) });
  const snapX = useTransform(gazeX, (v) => Math.round(v / PX) * PX);
  const snapY = useTransform(gazeY, (v) => Math.round(v / PX) * PX);
  const beak = shape.feathers === "var(--series-2)" ? "var(--owl-pupil)" : "var(--highlight)";
  const fills: Record<string, string> = { b: shape.feathers, d: shape.feathers, w: "var(--owl-eye)", k: beak };
  const head = Object.fromEntries([...EAR_ROWS[shape.ears], ...HEAD].map((row, y) => [y, row]));

  return (
    <svg
      ref={ref}
      viewBox="0 0 64 64"
      aria-hidden
      data-slot="owl"
      data-mood={mood}
      data-still={still ? "" : undefined}
      className={cn("shrink-0 overflow-visible", className)}
      style={{ "--owl-blink": `${shape.blink.every}s`, "--owl-blink-offset": `-${shape.blink.offset}s`, ...style } as CSSProperties}
    >
      <g className="owl-body" shapeRendering="crispEdges">
        <Pixels rows={head} top={0} fill={(c) => fills[c] ?? null} />
        <Pixels rows={head} top={0} fill={(c) => (c === "d" ? "var(--owl-pupil)" : null)} opacity={() => 0.22} />
        <Pixels
          rows={MARKING_ROWS[shape.marking]}
          top={0}
          fill={(c) => (c === "m" ? "var(--owl-pupil)" : c === "e" ? "var(--owl-eye)" : null)}
          opacity={(c) => (c === "m" ? 0.25 : 0.4)}
        />
        <AnimatePresence initial={false} mode="popLayout">
          <motion.g
            key={mood}
            initial={{ scaleY: 0.1, opacity: 0 }}
            animate={{ scaleY: 1, opacity: 1 }}
            transition={{ duration: still ? 0 : 0.24, ease: [0.22, 1, 0.36, 1] }}
            style={{ originY: `${(EYE_ROW + 2) * PX}px` }}
          >
            <Eyes mood={mood} feathers={shape.feathers} gaze={[snapX, snapY]} />
          </motion.g>
        </AnimatePresence>
      </g>
      {mood === "asleep" && !still ? (
        <>
          <PixelZ x={52} y={4} p={2.4} className="owl-z" />
          <PixelZ x={59} y={-4} p={1.8} className="owl-z owl-z-late" />
        </>
      ) : null}
    </svg>
  );
}

/** An agent's own owl, in its mode. */
export function AgentOwl({ agent, still, className }: { agent: Pick<Agent, "agent_id" | "mode">; still?: boolean; className?: string }) {
  return <Owl seed={agent.agent_id} mood={moodFor(agent.mode)} still={still} className={className} />;
}
