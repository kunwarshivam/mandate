import { cn } from "@/lib/utils";
import type { Environment } from "@/fixtures/types";

/**
 * Shown wherever an agent, a connection, or money appears (brief §5, rule 8). Paper is the account's
 * state, so it wears the account's lapis, hatched.
 */
export function EnvironmentBadge({ environment, className }: { environment: Environment; className?: string }) {
  if (environment === "live") {
    return (
      <span data-slot="environment-badge" className={cn("inline-flex h-8 items-center bg-foreground px-2.5 text-background label-caps", className)}>
        LIVE
      </span>
    );
  }
  return (
    <span
      data-slot="environment-badge"
      className={cn("hatch inline-flex h-8 items-center gap-1.5 border-2 border-lapis bg-card px-2 text-caption whitespace-nowrap text-foreground", className)}
    >
      <span className="label-caps">PAPER</span>
      <span aria-hidden className="max-[25rem]:hidden">·</span>
      <span className="font-medium max-[25rem]:sr-only">simulated funds</span>
    </span>
  );
}
