import { OwlheadLockup, OwlheadMark } from "@/components/brand/Logo";
import { NAVY } from "@/components/brand/palette";
import { cn } from "@/lib/utils";

/**
 * The Owlhead brand in the light header (DEC-203): navy, the mark alone on a phone, the lockup from
 * `sm` up. Decorative, because the link around it carries the name.
 */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: NAVY }}>
      <OwlheadMark title="" className="h-7 w-auto sm:hidden" />
      <OwlheadLockup title="" className="hidden h-7 w-auto sm:block" />
    </span>
  );
}
