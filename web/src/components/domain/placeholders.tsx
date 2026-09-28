import { cn } from "@/lib/utils";

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
      className={cn("inline-block rounded-sm border border-dashed border-muted-foreground px-1.5 py-0.5 font-mono text-label font-normal text-muted-foreground", className)}
    >
      {NAMES[name]}
    </span>
  );
}

/** Marks fixture values so no one reads them as a record. */
export function FixtureTag({ className }: { className?: string }) {
  return (
    <span data-slot="fixture-tag" className={cn("inline-flex h-5 shrink-0 items-center rounded-sm border border-dashed border-muted-foreground px-1.5 text-label text-muted-foreground", className)}>
      Fixture data
    </span>
  );
}
