"use client";

import { type CSSProperties, useEffect, useRef } from "react";
import { AnimatePresence, motion, useReducedMotion, useSpring } from "motion/react";
import type { Agent, AgentMode } from "@/fixtures/types";
import { cn } from "@/lib/utils";

/**
 * Each agent is an owl head (DEC-217). Its shape comes from its ID, so an agent keeps its face on
 * every screen and across sessions. Its eyes say its mode and nothing else: open when it trades,
 * half closed on exits only, asleep when paused, shut when stopped. Nothing about it follows P&L,
 * so an owl never cheers a gain or sulks at a loss (DESIGN.md, "No gamification").
 *
 * An awake owl blinks on its own rhythm and its pupils follow the pointer; a sleeping owl breathes;
 * a stopped owl is still. Under reduced motion every owl is still.
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

export interface OwlShape {
  /** Where the ear tufts end: how far out and how high. */
  tuft: { x: number; y: number; round: boolean };
  /** The radius of each facial disc. */
  disc: number;
  /** Where the pupils look, in viewBox units. */
  gaze: number;
  marking: "none" | "brow" | "speckles";
  /** The ring around each pupil, the owl's one colour. */
  iris: (typeof IRIS)[number];
  /** Seconds between blinks, and how far into that cycle this owl starts, so owls never blink together. */
  blink: { every: number; offset: number };
}

const TUFT_X = [5, 8, 11] as const;
const TUFT_Y = [2, 5, 8] as const;
const DISC = [10, 11, 12] as const;
const GAZE = [-1.2, 0, 1.2] as const;
const MARKING = ["none", "brow", "speckles"] as const;
const IRIS = ["var(--highlight)", "var(--series-3)", "var(--series-4)"] as const;
const BLINK_EVERY = [3.8, 4.6, 5.4, 6.2] as const;

export function owlShape(id: string): OwlShape {
  const s = owlSeed(id);
  const pick = <T,>(list: readonly T[], shift: number) => list[(s >>> shift) % list.length];
  const every = pick(BLINK_EVERY, 20);
  return {
    tuft: { x: pick(TUFT_X, 0), y: pick(TUFT_Y, 3), round: ((s >>> 6) & 1) === 1 },
    disc: pick(DISC, 8),
    gaze: pick(GAZE, 11),
    marking: pick(MARKING, 14),
    iris: pick(IRIS, 17),
    blink: { every, offset: Math.round((((s >>> 23) % 100) / 100) * every * 10) / 10 },
  };
}

/** The head, drawn clockwise from the chin, with a tuft on each side mirrored about the middle. */
function headPath({ x, y, round }: OwlShape["tuft"]): string {
  const [lx, rx] = [x, 64 - x];
  const left = round ? `Q${lx - 2} ${y + 6} ${lx} ${y} Q${lx + 7} ${y + 2} 22 11` : `L${lx} ${y} L22 11`;
  const right = round ? `Q${rx - 7} ${y + 2} ${rx} ${y} Q${rx + 2} ${y + 6} 50 14` : `L${rx} ${y} L50 14`;
  return `M32 61 C15 61 7 50 7 37 C7 27 10 19 14 14 ${left} C25 10 28 9.5 32 9.5 C36 9.5 39 10 42 11 ${right} C54 19 57 27 57 37 C57 50 49 61 32 61Z`;
}

/** How far a pupil may travel from the middle of its disc, as a share of the disc's radius. */
const REACH = 0.24;
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

function Eye({ cx, cy, r, iris, mood }: { cx: number; cy: number; r: number; iris: string; mood: OwlMood }) {
  switch (mood) {
    case "awake":
      return (
        <g className="owl-blink">
          <circle cx={cx} cy={cy} r={r * 0.62} fill={iris} />
          <circle cx={cx} cy={cy} r={r * 0.4} fill="var(--foreground)" />
          <circle cx={cx + r * 0.16} cy={cy - r * 0.16} r={r * 0.13} fill="var(--card)" />
        </g>
      );
    case "focused":
      return (
        <g className="owl-blink">
          <circle cx={cx} cy={cy + r * 0.1} r={r * 0.6} fill={iris} />
          <circle cx={cx} cy={cy + r * 0.12} r={r * 0.38} fill="var(--foreground)" />
        </g>
      );
    case "asleep":
      return <path d={`M${cx - r * 0.5} ${cy - r * 0.05} Q${cx} ${cy + r * 0.45} ${cx + r * 0.5} ${cy - r * 0.05}`} fill="none" stroke="var(--foreground)" strokeWidth={2.2} strokeLinecap="round" />;
    case "stopped":
      return <path d={`M${cx - r * 0.5} ${cy + r * 0.1} H${cx + r * 0.5}`} fill="none" stroke="var(--foreground)" strokeWidth={2.4} strokeLinecap="round" />;
    default: {
      const unhandled: never = mood;
      throw new Error(`unhandled owl mood ${String(unhandled)}`);
    }
  }
}

/** Open eyes look about; closed ones stay put. */
function looks(mood: OwlMood): boolean {
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
  const r = shape.disc;
  const [lx, rx, cy] = [32 - r * 0.86, 32 + r * 0.86, 35];
  const ref = useRef<SVGSVGElement>(null);
  // Reduced motion only gates the pointer here; the stylesheet stops the rest, so the server's
  // markup, which cannot know the preference, still matches the client's.
  const reduced = useReducedMotion() ?? false;
  const tracking = !still && !reduced && looks(mood);
  const gazeX = useSpring(shape.gaze, SPRING);
  const gazeY = useSpring(0, SPRING);

  useEffect(() => {
    if (!tracking) {
      gazeX.jump(shape.gaze);
      gazeY.jump(0);
      return;
    }
    const reach = r * REACH;
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
    const rest = () => {
      gazeX.set(shape.gaze);
      gazeY.set(0);
    };
    document.documentElement.addEventListener("pointerleave", rest);
    return () => {
      stop();
      document.documentElement.removeEventListener("pointerleave", rest);
    };
  }, [tracking, r, shape.gaze, gazeX, gazeY]);

  const eyes = (
    <AnimatePresence initial={false} mode="popLayout">
      <motion.g
        key={mood}
        initial={{ scaleY: 0.1, opacity: 0 }}
        animate={{ scaleY: 1, opacity: 1 }}
        transition={{ duration: still ? 0 : 0.32, ease: [0.22, 1, 0.36, 1] }}
        style={{ originY: `${cy}px` }}
      >
        <motion.g style={{ x: gazeX, y: gazeY }}>
          <Eye cx={lx} cy={cy} r={r} iris={shape.iris} mood={mood} />
          <Eye cx={rx} cy={cy} r={r} iris={shape.iris} mood={mood} />
        </motion.g>
        {mood === "focused" ? (
          <g fill="var(--foreground)">
            <path d={`M${lx - r} ${cy} A${r} ${r} 0 0 1 ${lx + r} ${cy} Z`} />
            <path d={`M${rx - r} ${cy} A${r} ${r} 0 0 1 ${rx + r} ${cy} Z`} />
          </g>
        ) : null}
      </motion.g>
    </AnimatePresence>
  );

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
      <g className="owl-body">
        <path d={headPath(shape.tuft)} fill="var(--foreground)" />
        {shape.marking === "brow" ? <path d="M24 18 L32 23 L40 18" fill="none" stroke="var(--lapis-line)" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" /> : null}
        {shape.marking === "speckles" ? (
          <g fill="var(--lapis-line)">
            <circle cx={26} cy={53} r={1.4} />
            <circle cx={32} cy={55.5} r={1.4} />
            <circle cx={38} cy={53} r={1.4} />
          </g>
        ) : null}
        <circle cx={lx} cy={cy} r={r} fill="var(--card)" />
        <circle cx={rx} cy={cy} r={r} fill="var(--card)" />
        {eyes}
        <path d={`M32 ${cy + r * 0.45} L35 ${cy + r * 0.45 + 4} L32 ${cy + r * 0.45 + 8.5} L29 ${cy + r * 0.45 + 4} Z`} fill="var(--highlight)" stroke="var(--foreground)" strokeWidth={0.8} strokeLinejoin="round" />
      </g>
      {mood === "asleep" && !still ? (
        <g fill="var(--muted-foreground)" fontWeight={700} fontFamily="inherit">
          <text className="owl-z" x={51} y={16} fontSize={12}>
            z
          </text>
          <text className="owl-z owl-z-late" x={58} y={7} fontSize={9}>
            z
          </text>
        </g>
      ) : null}
    </svg>
  );
}

/** An agent's own owl, in its mode. */
export function AgentOwl({ agent, still, className }: { agent: Pick<Agent, "agent_id" | "mode">; still?: boolean; className?: string }) {
  return <Owl seed={agent.agent_id} mood={moodFor(agent.mode)} still={still} className={className} />;
}
