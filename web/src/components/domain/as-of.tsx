import { cn } from "@/lib/utils";
import { ago, clock } from "@/lib/format";

/**
 * "3 min ago", re-rendered every tick. Tabular figures and a width that holds "59 min ago", so a
 * changing age never moves the text around it.
 */
export function Age({ at, now }: { at: string; now: string }) {
  return (
    <span data-slot="age" className="inline-block min-w-[10ch] whitespace-nowrap tabular">
      {ago(at, now)}
    </span>
  );
}

/**
 * A system state's word, "Stale" or "Down": outlined in the text colour, never filled, because system
 * states carry no meaning colour (the Meaning Rule). Screen readers hear the word and a pause. The
 * chip is positioned so the visually hidden pause stays inside it, and inside a strip that scrolls
 * sideways, rather than widening the page.
 */
export function StateChip({ children, className }: { children: string; className?: string }) {
  return (
    <span data-slot="state-chip" className={cn("relative rounded-sm border border-foreground px-1.5 text-label", className)}>
      {children}
      <span className="sr-only">:</span>
    </span>
  );
}

/** "as of 14:02:11, 3 min ago". A stale value keeps its figure and says how old it is. */
export function AsOf({ at, now, stale, className }: { at: string; now: string; stale?: boolean; className?: string }) {
  return (
    <span data-stale={stale ? "true" : undefined} className={cn("inline-flex flex-wrap items-center gap-1.5 text-caption text-muted-foreground", stale && "font-medium text-foreground", className)}>
      {stale ? (
        <>
          <StateChip>Stale</StateChip>{" "}
        </>
      ) : null}
      <span className="whitespace-nowrap tabular">
        as of <time dateTime={at}>{clock(at)}</time>, <Age at={at} now={now} />
      </span>
    </span>
  );
}
