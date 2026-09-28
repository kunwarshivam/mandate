import { OwlheadLockup, OwlheadMark } from "@/components/brand/Logo";
import { NAVY } from "@/lib/brand-palette";
import { cn } from "@/lib/utils";

/**
 * The Owlhead mark in the top header (DEC-203), in navy; off-white in dark mode (`--logo`). It shows below `lg` only: from `lg`
 * up the sidebar header carries the brand, so the top header never repeats it. Decorative, because
 * the link around it carries the name.
 */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: `var(--logo, ${NAVY})` }}>
      <OwlheadMark title="" className="h-7 w-auto" />
    </span>
  );
}

/**
 * The Owlhead brand in the light sidebar header, in navy: the lockup when the sidebar is open, the
 * mark alone when it collapses to icons. Never off-white on a navy block. Decorative, like `Wordmark`.
 */
export function SidebarBrand({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: `var(--logo, ${NAVY})` }}>
      <OwlheadLockup title="" className="h-7 w-auto group-data-[state=collapsed]/sidebar:hidden" />
      <OwlheadMark title="" className="hidden h-7 w-auto group-data-[state=collapsed]/sidebar:block" />
    </span>
  );
}
