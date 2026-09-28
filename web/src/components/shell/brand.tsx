import { OwlheadLockup, OwlheadMark } from "@/components/brand/Logo";
import { cn } from "@/lib/utils";

/**
 * The Owlhead mark in the top header (DEC-203, amended by DEC-204): ink on light, off-white on dark (`--logo`). It shows below `lg` only: from `lg`
 * up the sidebar header carries the brand, so the top header never repeats it. Decorative, because
 * the link around it carries the name.
 */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: "var(--logo)" }}>
      <OwlheadMark title="" className="h-7 w-auto" />
    </span>
  );
}

/**
 * The Owlhead brand in the sidebar header, ink on light and off-white on dark: the lockup when the
 * sidebar is open, the mark alone when it collapses to icons. Never reversed out of a block of
 * colour. Decorative, like `Wordmark`.
 */
export function SidebarBrand({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: "var(--logo)" }}>
      <OwlheadLockup title="" className="h-7 w-auto group-data-[state=collapsed]/sidebar:hidden" />
      <OwlheadMark title="" className="hidden h-7 w-auto group-data-[state=collapsed]/sidebar:block" />
    </span>
  );
}
