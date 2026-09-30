"use client";

import { type CSSProperties, type ReactNode, type Ref, useRef } from "react";
import { type MotionValue, motion } from "motion/react";
import { type OwlMood, Owl, looks, owlSeed, useGaze } from "@/components/domain/owl";
import { AGENT_IDS } from "@/fixtures/workspace";
import { cn } from "@/lib/utils";

/**
 * Directions for the agents' owls, side by side, to choose one (DEC-217). Each draws the same
 * agent from the same ID and speaks the same moods; they differ only in how they are drawn.
 * Feathers take the series hues and never gain or loss, so an owl never reads as P&L.
 */
type Direction = "ink" | "blob" | "spectacles" | "round" | "pixel";
/** "working" previews an agent at work, like a chat bot's "is working": eyes scan, the head bobs. */
type StudioMood = OwlMood | "working";

const DIRECTIONS: Array<{ id: Direction; name: string; note: string }> = [
  { id: "ink", name: "Ink", note: "Today's owl: an ink head with pale discs; its colour is only in the iris." },
  { id: "blob", name: "Blob", note: "Chunky and friendly, in the agent's own colour, with big eyes and wings." },
  { id: "spectacles", name: "Spectacles", note: "The blob with ringed eyes: the careful analyst." },
  { id: "round", name: "Round", note: "A circle and two ears: the simplest at 24 px." },
  { id: "pixel", name: "Pixel", note: "A 16 × 16 sprite, blinking a pixel at a time." },
];

const MOODS: StudioMood[] = ["awake", "working", "focused", "asleep", "stopped"];
const MOOD_LABEL: Record<StudioMood, string> = {
  awake: "Normal",
  working: "Working",
  focused: "Exits only",
  asleep: "Paused",
  stopped: "Stopped",
};

const FEATHERS = ["var(--series-1)", "var(--series-2)", "var(--series-3)", "var(--series-4)"] as const;
/** The fixture's agents, then sample IDs, to show how faces and colours vary. */
const AGENTS = [...Object.values(AGENT_IDS), "agt_01JB3KD7XC2M9QW4E6R8T0Y1ZN", "agt_01JB3KF3VB5N8PL2K4J6H9G0QM", "agt_01JB3KH9ZT1W3E5R7Y2U4I6O8P"];

function feathers(seed: string): (typeof FEATHERS)[number] {
  return FEATHERS[(owlSeed(seed) >>> 11) % FEATHERS.length];
}

function ownMood(mood: StudioMood): OwlMood {
  return mood === "working" ? "awake" : mood;
}

function Frame({ mood, seed, children, className, svgRef }: { mood: StudioMood; seed: string; children: ReactNode; className?: string; svgRef: Ref<SVGSVGElement> }) {
  const s = owlSeed(seed);
  const every = [3.8, 4.6, 5.4, 6.2][(s >>> 20) % 4];
  return (
    <svg
      ref={svgRef}
      viewBox="0 0 64 64"
      aria-hidden
      data-slot="owl"
      data-mood={ownMood(mood)}
      data-working={mood === "working" ? "" : undefined}
      className={cn("shrink-0 overflow-visible", className)}
      style={{ "--owl-blink": `${every}s`, "--owl-blink-offset": `-${((s >>> 23) % 10) / 2}s` } as CSSProperties}
    >
      {children}
    </svg>
  );
}

/** Two white eyes on a coloured face, in any mood. The lid on exits only is the face's own colour. */
function Eyes({ at, r, mood, face, gaze }: { at: Array<[number, number]>; r: number; mood: OwlMood; face: string; gaze: [MotionValue<number>, MotionValue<number>] }) {
  const pupils = (dy: number) => (
    <motion.g style={{ x: gaze[0], y: gaze[1] }}>
      <g className="owl-scan">
        {at.map(([x, y]) => (
          <g key={x} className="owl-blink">
            <circle cx={x} cy={y + dy} r={r * 0.5} fill="var(--owl-pupil)" />
            <circle cx={x + r * 0.2} cy={y + dy - r * 0.2} r={r * 0.17} fill="var(--owl-eye)" />
          </g>
        ))}
      </g>
    </motion.g>
  );
  const whites = at.map(([x, y]) => <circle key={x} cx={x} cy={y} r={r} fill="var(--owl-eye)" />);
  switch (mood) {
    case "awake":
      return (
        <>
          {whites}
          {pupils(0)}
        </>
      );
    case "focused":
      return (
        <>
          {whites}
          {pupils(r * 0.25)}
          {at.map(([x, y]) => (
            <path key={x} d={`M${x - r - 0.5} ${y - 0.5} A${r + 0.5} ${r + 0.5} 0 0 1 ${x + r + 0.5} ${y - 0.5} Z`} fill={face} />
          ))}
        </>
      );
    case "asleep":
      return (
        <>
          {whites}
          {at.map(([x, y]) => (
            <path key={x} d={`M${x - r * 0.55} ${y - r * 0.1} Q${x} ${y + r * 0.5} ${x + r * 0.55} ${y - r * 0.1}`} fill="none" stroke="var(--owl-pupil)" strokeWidth={2.4} strokeLinecap="round" />
          ))}
        </>
      );
    case "stopped":
      return (
        <>
          {whites}
          {at.map(([x, y]) => (
            <path key={x} d={`M${x - r * 0.55} ${y} H${x + r * 0.55}`} fill="none" stroke="var(--owl-pupil)" strokeWidth={2.6} strokeLinecap="round" />
          ))}
        </>
      );
    default: {
      const unhandled: never = mood;
      throw new Error(`unhandled owl mood ${String(unhandled)}`);
    }
  }
}

function Beak({ x = 32, y }: { x?: number; y: number }) {
  return <path d={`M${x - 3.2} ${y} L${x + 3.2} ${y} L${x} ${y + 5} Z`} fill="var(--highlight)" stroke="var(--owl-pupil)" strokeWidth={0.9} strokeLinejoin="round" />;
}

function Snooze({ mood }: { mood: StudioMood }) {
  if (mood !== "asleep") return null;
  return (
    <g fill="var(--muted-foreground)" fontWeight={700} fontFamily="inherit">
      <text className="owl-z" x={51} y={14} fontSize={12}>
        z
      </text>
      <text className="owl-z owl-z-late" x={58} y={5} fontSize={9}>
        z
      </text>
    </g>
  );
}

function BlobOwl({ seed, mood, rings, className }: { seed: string; mood: StudioMood; rings?: boolean; className?: string }) {
  const ref = useRef<SVGSVGElement>(null);
  const face = feathers(seed);
  const own = ownMood(mood);
  const gaze = useGaze(ref, { rest: 0, reach: 2.4, looking: looks(own) && mood !== "working" });
  const at: Array<[number, number]> = [
    [21.5, 33],
    [42.5, 33],
  ];
  return (
    <Frame mood={mood} seed={seed} svgRef={ref} className={className}>
      <g className="owl-body">
        <ellipse cx={8.5} cy={42} rx={5} ry={10} fill={face} />
        <ellipse cx={55.5} cy={42} rx={5} ry={10} fill={face} />
        <ellipse cx={8.5} cy={42} rx={5} ry={10} fill="var(--owl-pupil)" opacity={0.22} />
        <ellipse cx={55.5} cy={42} rx={5} ry={10} fill="var(--owl-pupil)" opacity={0.22} />
        <path d="M10 22 Q9 12 11 5 Q18 8 24 12.5 Q32 11 40 12.5 Q46 8 53 5 Q55 12 54 22 Q59 30 59 40 Q59 59 32 59 Q5 59 5 40 Q5 30 10 22Z" fill={face} />
        {rings ? (
          <g fill="none" stroke="var(--owl-eye)" strokeWidth={2.2}>
            {at.map(([x, y]) => (
              <circle key={x} cx={x} cy={y} r={9.9} />
            ))}
            <path d="M31.4 32 Q32 30.8 32.6 32" strokeLinecap="round" />
          </g>
        ) : null}
        <Eyes at={at} r={rings ? 7.6 : 10} mood={own} face={face} gaze={gaze} />
        <Beak y={40.5} />
      </g>
      <Snooze mood={mood} />
    </Frame>
  );
}

function RoundOwl({ seed, mood, className }: { seed: string; mood: StudioMood; className?: string }) {
  const ref = useRef<SVGSVGElement>(null);
  const face = feathers(seed);
  const own = ownMood(mood);
  const gaze = useGaze(ref, { rest: 0, reach: 2.2, looking: looks(own) && mood !== "working" });
  return (
    <Frame mood={mood} seed={seed} svgRef={ref} className={className}>
      <g className="owl-body">
        <path d="M12 22 L10 6 L26 13 Z M52 22 L54 6 L38 13 Z" fill={face} strokeLinejoin="round" />
        <circle cx={32} cy={36} r={25} fill={face} />
        <Eyes
          at={[
            [22, 33],
            [42, 33],
          ]}
          r={9.5}
          mood={own}
          face={face}
          gaze={gaze}
        />
        <Beak y={41} />
      </g>
      <Snooze mood={mood} />
    </Frame>
  );
}

/**
 * The sprite, row by row: b face, w eye white, k beak, . empty. Pupils and lids are drawn over
 * it, so a blink swaps whole pixels.
 */
const SPRITE = [
  "..b..........b..",
  "..bb........bb..",
  "..bbbbbbbbbbbb..",
  ".bbbbbbbbbbbbbb.",
  ".bwwwwbbbbwwwwb.",
  "bbwwwwbbbbwwwwbb",
  "bbwwwwbbbbwwwwbb",
  "bbwwwwbbbbwwwwbb",
  ".bbbbbbkkbbbbbb.",
  ".bbbbbbkkbbbbbb.",
  ".bbbbbbbbbbbbbb.",
  "..bbbbbbbbbbbb..",
  "..bbbbbbbbbbbb..",
  "...bbbbbbbbbb...",
  "....bb....bb....",
  "................",
];
const PX = 4;

function PixelOwl({ seed, mood, className }: { seed: string; mood: StudioMood; className?: string }) {
  const ref = useRef<SVGSVGElement>(null);
  const face = feathers(seed);
  const own = ownMood(mood);
  const gaze = useGaze(ref, { rest: 0, reach: PX / 2, looking: looks(own) && mood !== "working" });
  const fill = { b: face, w: "var(--owl-eye)", k: "var(--highlight)" } as const;
  const cells = SPRITE.flatMap((row, y) =>
    [...row].flatMap((c, x) => (c === "." ? [] : [<rect key={`${x}-${y}`} x={x * PX} y={y * PX} width={PX} height={PX} fill={fill[c as keyof typeof fill]} />])),
  );
  const eyes: Array<[number, number]> = [
    [3, 5],
    [11, 5],
  ];
  const pixelEyes = () => {
    switch (own) {
      case "awake":
      case "focused":
        return (
          <>
            <motion.g style={{ x: gaze[0], y: gaze[1] }}>
              <g className="owl-scan">
                {eyes.map(([x, y]) => (
                  <g key={x} className="owl-blink">
                    <rect x={x * PX} y={(y + (own === "focused" ? 1 : 0)) * PX} width={PX * 2} height={PX * 2} fill="var(--owl-pupil)" />
                  </g>
                ))}
              </g>
            </motion.g>
            {own === "focused" ? eyes.map(([x, y]) => <rect key={x} x={(x - 1) * PX} y={(y - 1) * PX} width={PX * 4} height={PX * 2} fill={face} />) : null}
          </>
        );
      case "asleep":
        return eyes.map(([x, y]) => <rect key={x} x={(x - 0.5) * PX} y={(y + 1.5) * PX} width={PX * 3} height={PX} fill="var(--owl-pupil)" />);
      case "stopped":
        return eyes.map(([x, y]) => <rect key={x} x={(x - 0.5) * PX} y={(y + 0.5) * PX} width={PX * 3} height={PX} fill="var(--owl-pupil)" />);
      default: {
        const unhandled: never = own;
        throw new Error(`unhandled owl mood ${String(unhandled)}`);
      }
    }
  };
  return (
    <Frame mood={mood} seed={seed} svgRef={ref} className={className}>
      <g className="owl-body" shapeRendering="crispEdges">
        {cells}
        {pixelEyes()}
      </g>
      <Snooze mood={mood} />
    </Frame>
  );
}

function StudioOwl({ direction, seed, mood, className }: { direction: Direction; seed: string; mood: StudioMood; className?: string }) {
  switch (direction) {
    case "ink":
      return (
        <span data-working={mood === "working" ? "" : undefined} className="contents">
          <Owl seed={seed} mood={ownMood(mood)} className={className} />
        </span>
      );
    case "blob":
      return <BlobOwl seed={seed} mood={mood} className={className} />;
    case "spectacles":
      return <BlobOwl seed={seed} mood={mood} rings className={className} />;
    case "round":
      return <RoundOwl seed={seed} mood={mood} className={className} />;
    case "pixel":
      return <PixelOwl seed={seed} mood={mood} className={className} />;
    default: {
      const unhandled: never = direction;
      throw new Error(`unhandled owl direction ${String(unhandled)}`);
    }
  }
}

export function OwlStudio() {
  return (
    <div data-slot="owl-studio" className="grid gap-4">
      {DIRECTIONS.map((d, i) => (
        <article key={d.id} data-owl-direction={d.id} className="grid gap-5 rounded-lg border border-border bg-card p-5 lg:grid-cols-[14rem_minmax(0,1fr)_auto] lg:items-center">
          <div className="grid gap-1">
            <h3 className="font-semibold">
              <span className="font-mono text-muted-foreground tabular">{String.fromCharCode(65 + i)}</span> {d.name}
            </h3>
            <p className="text-sm text-muted-foreground">{d.note}</p>
          </div>
          <div className="grid gap-4">
            <ul aria-label={`${d.name}: one owl per agent`} className="flex flex-wrap items-end gap-4">
              {AGENTS.map((id) => (
                <li key={id}>
                  <StudioOwl direction={d.id} seed={id} mood="awake" className="size-16" />
                </li>
              ))}
            </ul>
            <ul aria-label={`${d.name}: each mode`} className="flex flex-wrap gap-x-5 gap-y-3">
              {MOODS.map((m) => (
                <li key={m} className="grid justify-items-center gap-1.5">
                  <StudioOwl direction={d.id} seed={AGENTS[1]} mood={m} className="size-12" />
                  <span className="text-caption text-muted-foreground">{MOOD_LABEL[m]}</span>
                </li>
              ))}
            </ul>
          </div>
          <div aria-label={`${d.name} at the sizes the app uses`} role="group" className="flex items-end gap-3 lg:flex-col lg:items-center">
            <StudioOwl direction={d.id} seed={AGENTS[0]} mood="awake" className="size-10" />
            <StudioOwl direction={d.id} seed={AGENTS[0]} mood="awake" className="size-8" />
            <StudioOwl direction={d.id} seed={AGENTS[0]} mood="awake" className="size-6" />
            <p className="flex items-center gap-2 text-sm">
              <StudioOwl direction={d.id} seed={AGENTS[2]} mood="working" className="size-6" />
              <span>
                <span className="font-medium">Agent 3</span> <span className="text-muted-foreground">is working</span>
              </span>
            </p>
          </div>
        </article>
      ))}
    </div>
  );
}
