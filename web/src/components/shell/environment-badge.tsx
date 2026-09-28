import { cn } from "cn";
import type { Environment } from "@/fixtures/types";

/** Shown wherever an agent, a connection, or money appears (brief §5, rule 8). */
export function EnvironmentBadge({ environment, className }: { environment: Environment; className?: string }) {
  if (environment === "live") {
    return (
      <span data-slot="environment-badge" className={cn("inline-flex h-7 items-center rounded-md bg-foreground px-2.5 text-caption font-semibold text-background", className)}>
        LIVE
      </span>
    );
  }
  return (
    <span
      data-slot="environment-badge"
      className={cn(
        "hatch inline-flex h-7 items-center gap-1.5 rounded-md border border-persimmon/60 bg-card px-2.5 text-caption whitespace-nowrap text-foreground",
        className,
      )}
    >
      <span className="font-semibold tracking-wide">PAPER</span>
      <span aria-hidden>·</span>
      <span>simulated funds</span>
    </span>
  );
}
