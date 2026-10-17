import { cn } from "@/lib/utils";
import type { ShownEnvironment } from "@/fixtures/types";

/**
 * Shown wherever an agent, a connection, or money appears (brief §5, rule 8). Paper is the account's
 * state, so it wears the account's lapis, hatched. Over the workspace API an environment nothing names
 * is "not reported", never paper (DEC-1096), so a live account is never labelled paper.
 */
export function EnvironmentBadge({ environment, className }: { environment: ShownEnvironment; className?: string }) {
  if (environment === "unknown") {
    return (
      <span
        data-slot="environment-badge"
        data-environment="unknown"
        className={cn("inline-flex h-8 items-center rounded-md border border-border px-3 text-label font-semibold whitespace-nowrap text-foreground", className)}
      >
        ENVIRONMENT NOT REPORTED
      </span>
    );
  }
  if (environment === "live") {
    return (
      <span data-slot="environment-badge" className={cn("inline-flex h-8 items-center rounded-md bg-foreground px-3 text-label font-semibold text-background", className)}>
        LIVE
      </span>
    );
  }
  return (
    <span
      data-slot="environment-badge"
      className={cn("hatch inline-flex h-8 items-center gap-1.5 rounded-md border border-lapis bg-(--paper) px-3 text-caption whitespace-nowrap text-foreground", className)}
    >
      <span className="text-label font-semibold tracking-wide text-lapis">PAPER</span>
      <span aria-hidden className="max-[30rem]:hidden">·</span>
      <span className="max-[30rem]:sr-only">simulated funds</span>
    </span>
  );
}
