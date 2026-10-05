import { BrandOwl } from "@/components/brand/brand-owl";
import { Owl, type OwlMood } from "@/components/domain/owl";
import { cn } from "@/lib/utils";
import { PIXEL, SUNKEN } from "./letter";
import { PERCH } from "./perch";

/** A row of agents on a branch, under the headline. Their open eyes follow the pointer. */
export function Perch() {
  return (
    <div aria-hidden data-slot="owl-perch" className="grid justify-items-center pt-2">
      <div className="flex items-end gap-3 px-3 sm:gap-5">
        {PERCH.map(({ seed, mood }) => (
          <Owl key={seed} seed={seed} mood={mood} className="size-12 sm:size-16" />
        ))}
      </div>
      <div className="h-2 w-full max-w-[26rem] bg-foreground" />
    </div>
  );
}

const MODES: Array<{ mood: OwlMood; name: string; means: string }> = [
  { mood: "awake", name: "Trading", means: "Inside your rules" },
  { mood: "focused", name: "Only selling", means: "Closing, not opening" },
  { mood: "asleep", name: "Paused", means: "Waiting for you" },
  { mood: "stopped", name: "Stopped", means: "Ended for good" },
];

/** Each agent is an owl, and its eyes show what it may do right now. */
export function ModeChart({ className }: { className?: string }) {
  return (
    <figure data-slot="owl-modes" className={cn(SUNKEN, "grid gap-3 bg-card p-4", className)}>
      <ul className="grid grid-cols-2 gap-x-4 gap-y-5 sm:grid-cols-4">
        {MODES.map((m) => (
          <li key={m.mood} className="grid justify-items-center gap-1 text-center">
            <Owl seed="agt_01JB3K9P2H6SD4F8G1E3W7XYZB" mood={m.mood} className="size-12" />
            <span className={cn(PIXEL, "pt-1 text-[1.0625rem] leading-tight")}>{m.name}</span>
            <span className="text-sm leading-snug text-muted-foreground">{m.means}</span>
          </li>
        ))}
      </ul>
      <figcaption className="text-sm text-muted-foreground">Each agent is an owl. Its eyes show what it may do right now, and nothing about how it&apos;s doing.</figcaption>
    </figure>
  );
}

/** A 16 px brand owl for a window's title bar, one screen pixel to a sprite pixel. */
export function TitleOwl() {
  return <BrandOwl className="size-4" />;
}
