import { cn } from "@/lib/utils";
import { ago, clock } from "@/lib/format";

/** "as of 14:02:11, 3 min ago". A stale value keeps its figure and says how old it is. */
export function AsOf({ at, now, stale, className }: { at: string; now: string; stale?: boolean; className?: string }) {
  return (
    <span data-stale={stale ? "true" : undefined} className={cn("inline-flex flex-wrap items-center gap-1.5 text-caption text-muted-foreground", stale && "font-medium text-foreground", className)}>
      {stale ? (
        <>
          <span className="border-2 border-foreground px-1 label-caps">
            Stale<span className="sr-only">:</span>
          </span>{" "}
        </>
      ) : null}
      <span>
        as of <time dateTime={at}>{clock(at)}</time>, {ago(at, now)}
      </span>
    </span>
  );
}
