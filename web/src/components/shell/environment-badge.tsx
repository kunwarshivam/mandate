import { cn } from "@/lib/utils";
import type { Environment } from "@/fixtures/types";

/**
 * Shown wherever an agent, a connection, or money appears (brief §5, rule 8). Paper is the account's
 * state, so it wears the account's lapis, hatched.
 */
export function EnvironmentBadge({ environment, className }: { environment: Environment; className?: string }) {
  if (environment === "live") {
    return (
      <span data-slot="environment-badge" className={cn("inline-flex h-8 items-center rounded-full bg-foreground px-3 text-label font-semibold text-background", className)}>
        LIVE
      </span>
    );
  }
  return (
    <span
      data-slot="environment-badge"
      className={cn("hatch inline-flex h-8 items-center gap-1.5 rounded-full border border-lapis bg-card px-3 text-caption whitespace-nowrap text-foreground", className)}
    >
      <span className="text-label font-semibold tracking-wide text-lapis">PAPER</span>
      <span aria-hidden className="max-[30rem]:hidden">·</span>
      <span className="max-[30rem]:sr-only">simulated funds</span>
    </span>
  );
}
