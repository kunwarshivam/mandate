import { cn } from "@/lib/utils";
import type { FieldProvenance } from "@/fixtures/types";
import { PROVENANCE_LABEL, isPlatformAuthored } from "@/lib/labels";

/**
 * Who wrote a mandate field. What the platform authored is dashed, so it reads as a suggestion
 * rather than something you said; shape carries it, not a fifth meaning colour.
 */
export function ProvenanceBadge({ provenance, className }: { provenance: FieldProvenance | undefined; className?: string }) {
  if (!provenance) return null;
  const platform = isPlatformAuthored(provenance.provenance);
  return (
    <span
      data-slot="provenance-badge"
      data-provenance={provenance.provenance}
      className={cn(
        "inline-flex h-6 w-fit shrink-0 items-center px-1.5 text-caption whitespace-nowrap",
        platform ? "border border-dashed border-foreground font-semibold text-foreground" : "border border-border text-muted-foreground",
        className,
      )}
    >
      {PROVENANCE_LABEL[provenance.provenance]}
    </span>
  );
}
