import { cn } from "@/lib/utils";

/**
 * The Owlhead wordmark (DEC-201): the name set in the Placard display face, no symbol. The M mark
 * no longer fits the name, and a new mark waits for a founder-approved design.
 */
export function Wordmark({ className }: { className?: string }) {
  return <span className={cn("font-display leading-none font-extrabold uppercase", className)}>Owlhead</span>;
}
