import { OwlheadLockup, OwlheadMark } from "@/components/brand/Logo";
import { cn } from "@/lib/utils";

/**
 * The Owlhead brand in the top header (DEC-203, amended by DEC-204): ink on light, off-white on dark
 * (`--logo`). The mark below `lg`, and from `lg` up, where no sidebar carries the brand, the lockup.
 * Decorative, because the link around it carries the name.
 */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: "var(--logo)" }}>
      <OwlheadMark title="" className="h-7 w-auto lg:hidden" />
      <OwlheadLockup title="" className="hidden h-7 w-auto lg:block" />
    </span>
  );
}

/**
 * The Owlhead brand in the header of the phone and tablet sheet, ink on light and off-white on dark.
 * Never reversed out of a block of colour. Decorative, like `Wordmark`.
 */
export function SidebarBrand({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: "var(--logo)" }}>
      <OwlheadLockup title="" className="h-7 w-auto" />
    </span>
  );
}
