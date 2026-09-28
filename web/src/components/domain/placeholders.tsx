import { cn } from "cn";
import { Badge } from "@/components/ui/badge";

/**
 * Compliance text appears only as a named placeholder until counsel drafts it (brief §5, rule 9;
 * DEC-79). These components are the only place the placeholder names are written.
 */
const NAMES = {
  performance: "[[DISCLOSURE-PERFORMANCE]]",
  hypothetical: "[[LEGEND-HYPOTHETICAL]]",
  retailAutoLive: "[[RETAIL-AUTO-LIVE]]",
} as const;

export function Placeholder({ name, className }: { name: keyof typeof NAMES; className?: string }) {
  return (
    <span
      data-placeholder={name}
      className={cn("inline-block rounded-xs border border-dashed border-border px-1.5 py-0.5 font-mono text-[0.6875rem] text-muted-foreground", className)}
    >
      {NAMES[name]}
    </span>
  );
}

/** Marks fixture values so no one reads them as a record. */
export function FixtureTag({ className }: { className?: string }) {
  return (
    <Badge variant="secondary" data-slot="fixture-tag" className={cn("rounded-xs text-[0.6875rem] text-muted-foreground", className)}>
      Fixture data
    </Badge>
  );
}
