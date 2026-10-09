import { Owl } from "@/components/domain/owl";
import { cn } from "@/lib/utils";
import { OwlheadWordmark } from "./Logo";

/**
 * The Owlhead logo (DEC-452): the product's own owl, awake, in the logo's colour, the wordmark's, and
 * never an agent's (DEC-739 item 2), since a second owl in an agent's colour reads as a second agent.
 * Decorative, because the link or button around it carries the name.
 * Its sprite is 16 pixels, so it stays crisp at 16, 32 and 48 px.
 */
export function BrandOwl({ still, className }: { still?: boolean; className?: string }) {
  return (
    <span data-slot="brand-owl" className="inline-flex shrink-0">
      <Owl seed="owlhead" mood="awake" feathers="var(--brand-owl)" beak="var(--brand-owl-beak)" still={still} className={className} />
    </span>
  );
}

/**
 * The owl and the wordmark, the wordmark half the owl's height. `wordmark="lg"` keeps only the owl
 * below `lg`, where the phone header has room for three things (DEC-207).
 */
export function BrandLockup({ wordmark = "always", className }: { wordmark?: "always" | "lg"; className?: string }) {
  return (
    <span data-slot="brand-lockup" className={cn("inline-flex items-center gap-1.5", className)} style={{ color: "var(--logo)" }}>
      <BrandOwl className="size-8" />
      <OwlheadWordmark title="" className={cn("h-4 w-auto", wordmark === "lg" && "hidden lg:block")} />
    </span>
  );
}
