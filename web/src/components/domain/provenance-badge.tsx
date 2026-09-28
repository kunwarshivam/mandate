import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import type { FieldProvenance } from "@/fixtures/types";
import { PROVENANCE_LABEL, isPlatformAuthored } from "@/lib/labels";

export function ProvenanceBadge({ provenance, className }: { provenance: FieldProvenance | undefined; className?: string }) {
  if (!provenance) return null;
  const platform = isPlatformAuthored(provenance.provenance);
  return (
    <Badge
      variant="outline"
      data-provenance={provenance.provenance}
      className={cn("text-[0.6875rem]", platform ? "border-orchid/50 text-orchid-text" : "text-muted-foreground", className)}
    >
      {PROVENANCE_LABEL[provenance.provenance]}
    </Badge>
  );
}
