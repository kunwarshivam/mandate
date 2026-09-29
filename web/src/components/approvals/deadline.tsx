import { cn } from "@/lib/utils";
import { clock, remaining, zoneLabel } from "@/lib/format";

function quantize(iso: string, seconds: number): string {
  const ms = Date.parse(iso);
  const stepped = ms - (ms % (seconds * 1000));
  return new Date(stepped).toISOString();
}

/**
 * The deadline as an absolute time and whole minutes remaining, in neutral type. It never pulses,
 * counts seconds, or changes colour as time runs out (PX-10). The count holds the width of
 * "(59 min left)", so the sentence never rewraps as it ticks down. A list row on a phone passes
 * `staticOnPhone` and keeps only the time; the request itself always states both (DEC-207).
 */
export function Deadline({ deadline, now, staticOnPhone = false, className }: { deadline: string; now: string; staticOnPhone?: boolean; className?: string }) {
  return (
    <p data-slot="deadline" className={cn("text-sm text-foreground", className)}>
      Skipped at{" "}
      <time dateTime={deadline} className="font-mono tabular">
        {clock(deadline)} {zoneLabel(deadline)}
      </time>{" "}
      if you do nothing{" "}
      <span data-slot="remaining" className={cn("inline-block min-w-[13ch] whitespace-nowrap text-muted-foreground tabular", staticOnPhone && "max-lg:hidden")}>
        ({remaining(deadline, quantize(now, 15))})
      </span>
    </p>
  );
}
