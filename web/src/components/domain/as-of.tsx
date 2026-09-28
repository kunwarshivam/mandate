import { cn } from "@/lib/utils";
import { ago, clock } from "@/lib/format";

/** "as of 14:02:11, 3 min ago". A stale value keeps its figure and says how old it is. */
export function AsOf({ at, now, stale, className }: { at: string; now: string; stale?: boolean; className?: string }) {
  return (
    <span data-stale={stale ? "true" : undefined} className={cn("text-caption text-muted-foreground", stale && "font-medium text-persimmon-text", className)}>
      {stale ? "Stale: " : ""}as of <time dateTime={at}>{clock(at)}</time>, {ago(at, now)}
    </span>
  );
}
