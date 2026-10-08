"use client";

import { type CSSProperties, type RefObject, useEffect, useRef } from "react";
import { AnimatePresence, type MotionValue, motion, useReducedMotion, useReducedMotionConfig, useSpring, useTransform } from "motion/react";
import type { Agent, AgentMode } from "@/fixtures/types";
import { cn } from "@/lib/utils";
import { EYE_ROW, type OwlMood, type OwlRect, PX, beakFor, bodyRects, eyeRects, owlShape } from "./owl-sprite";

export { FEATHERS, beakFor, owlRows, owlSeed, owlShape, type OwlMood, type OwlShape } from "./owl-sprite";

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
const MOOD: Record<AgentMode, OwlMood> = {
  normal: "awake",
  exits_only: "focused",
  paused: "asleep",
  stopped: "stopped",
};

export function moodFor(mode: AgentMode): OwlMood {
  return MOOD[mode];
}

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

function Rects({ rects }: { rects: OwlRect[] }) {
  return rects.map((r, i) => <rect key={i} x={r.x * PX} y={r.y * PX} width={r.w * PX} height={r.h * PX} fill={r.fill} opacity={r.opacity} />);
}

function Eyes({ mood, feathers, gaze }: { mood: OwlMood; feathers: string; gaze: [MotionValue<number>, MotionValue<number>] }) {
  const { open, fixed } = eyeRects(mood, feathers);
  return (
    <>
      {open.length > 0 ? (
        <motion.g style={{ x: gaze[0], y: gaze[1] }}>
          {open.map((eye, i) => (
            <g key={i} className="owl-blink">
              <Rects rects={eye} />
            </g>
          ))}
        </motion.g>
      ) : null}
      <Rects rects={fixed} />
    </>
  );
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
  feathers: featherOverride,
  beak: beakOverride,
  className,
  style,
}: {
  seed: string;
  mood: OwlMood;
  /** Draw it without any motion, as in a static specimen. */
  still?: boolean;
  /** A feather colour in place of the one the seed picks, for the brand owl. */
  feathers?: string;
  beak?: string;
  className?: string;
  style?: CSSProperties;
}) {
  const shape = owlShape(seed);
  const ref = useRef<SVGSVGElement>(null);
  const [gazeX, gazeY] = useGaze(ref, { reach: REACH, looking: !still && looks(mood) });
  const snapX = useTransform(gazeX, (v) => Math.round(v / PX) * PX);
  const snapY = useTransform(gazeY, (v) => Math.round(v / PX) * PX);
  const feathers = featherOverride ?? shape.feathers;
  const beak = beakOverride ?? beakFor(feathers);

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
        <Rects rects={bodyRects(shape, feathers, beak)} />
        <AnimatePresence initial={false} mode="popLayout">
          <motion.g
            key={mood}
            initial={{ scaleY: 0.1, opacity: 0 }}
            animate={{ scaleY: 1, opacity: 1 }}
            transition={{ duration: still ? 0 : 0.24, ease: [0.22, 1, 0.36, 1] }}
            style={{ originY: `${(EYE_ROW + 2) * PX}px` }}
          >
            <Eyes mood={mood} feathers={feathers} gaze={[snapX, snapY]} />
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

/** The egg on the sprite's own grid: an outline and a shell, two halves that part along row 9. */
const EGG_ROWS = ["......oooo......", ".....obbbbo.....", "....obbbbbbo....", "...obbbbbbbbo...", "...obbbbbbbbo...", "..obbbbbbbbbbo..", "..obbbbbbbbbbo..", "..obbbbbbbbbbo..", "..obbbbbbbbbbo..", "..obbbbbbbbbbo..", "..obbbbbbbbbbo..", "...obbbbbbbbo...", "...obbbbbbbbo...", "....obbbbbbo....", ".....oooooo.....", "................"] as const;
const EGG_CRACK = 9;

function eggRects(rows: readonly string[]): OwlRect[] {
  const out: OwlRect[] = [];
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x++) {
      const c = row[x];
      if (c === ".") continue;
      let w = 1;
      while (row[x + w] === c) w++;
      out.push({ x, y, w, h: 1, fill: c === "o" ? "var(--muted-foreground)" : "var(--muted)" });
      x += w - 1;
    }
  });
  return out;
}

const HATCH_MS = 600;

/**
 * A new agent's owl hatching, once, when the runtime has recorded it (DEC-505): the egg parts along
 * its crack, the top half lifting away and the bottom half sinking, and the owl rises into place
 * over 600ms. It answers the owner's act of creating the agent, never plays again, and under
 * reduced motion the owl is simply there.
 */
export function HatchingOwl({ seed, className }: { seed: string; className?: string }) {
  const reduced = useReducedMotionConfig() ?? false;
  const top = eggRects(EGG_ROWS.slice(0, EGG_CRACK));
  const bottom = eggRects(EGG_ROWS.map((row, y) => (y < EGG_CRACK ? "................" : row)));
  const ease = [0.22, 1, 0.36, 1] as const;
  return (
    <span data-slot="hatch" data-reduced={reduced ? "" : undefined} className={cn("relative inline-grid shrink-0 place-items-center", className)}>
      <motion.span
        className="col-start-1 row-start-1 size-full"
        initial={reduced ? false : { opacity: 0, scale: 0.85, y: 6 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        transition={{ duration: reduced ? 0 : HATCH_MS / 1000, delay: reduced ? 0 : 0.18, ease }}
      >
        <Owl seed={seed} mood="awake" className="size-full" />
      </motion.span>
      {reduced ? null : (
        <svg viewBox="0 0 64 64" aria-hidden data-slot="egg" className="pointer-events-none col-start-1 row-start-1 size-full overflow-visible" shapeRendering="crispEdges">
          <motion.g initial={{ opacity: 1, y: 0, rotate: 0 }} animate={{ opacity: 0, y: -22, rotate: -14 }} transition={{ duration: HATCH_MS / 1000, ease }} style={{ originX: "32px", originY: `${EGG_CRACK * PX}px` }}>
            <Rects rects={top} />
          </motion.g>
          <motion.g initial={{ opacity: 1, y: 0 }} animate={{ opacity: 0, y: 10 }} transition={{ duration: HATCH_MS / 1000, ease }}>
            <Rects rects={bottom} />
          </motion.g>
        </svg>
      )}
    </span>
  );
}

/** An agent's own owl, in its mode. */
export function AgentOwl({ agent, still, className }: { agent: Pick<Agent, "agent_id" | "mode">; still?: boolean; className?: string }) {
  return <Owl seed={agent.agent_id} mood={moodFor(agent.mode)} still={still} className={className} />;
}
