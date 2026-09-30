import { OwlheadLockup, OwlheadMark } from "@/components/brand/Logo";
import { cn } from "@/lib/utils";

/**
 * The Owlhead brand in the top header (DEC-203, amended by DEC-204): ink on light, off-white on dark
 * (`--logo`). The mark below `lg`, and from `lg` up, where no sidebar carries the brand, the lockup.
 * The lockup runs at its larger size from 100rem; below that the header's compact band, the smaller
 * size keeps room for the trail (`e2e/command-bar.spec.ts`). Decorative, because the link around it
 * carries the name.
 */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: "var(--logo)" }}>
      <OwlheadMark title="" className="h-9 w-auto lg:hidden" />
      <OwlheadLockup title="" className="hidden h-7 w-auto lg:block min-[100rem]:h-11" />
    </span>
  );
}
