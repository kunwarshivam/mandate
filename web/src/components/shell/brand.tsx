import { OwlheadLockup, OwlheadMark } from "@/components/brand/Logo";
import { NAVY } from "@/components/brand/palette";
import { cn } from "@/lib/utils";

/**
 * The Owlhead brand in the light header (DEC-203), in navy. Below `xl` the header has no width to
 * spare before the Stop control, so it shows the mark alone; the lockup needs about 20 px more than
 * the breadcrumbs can give up there. Decorative, because the link around it carries the name.
 */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center", className)} style={{ color: NAVY }}>
      <OwlheadMark title="" className="h-7 w-auto xl:hidden" />
      <OwlheadLockup title="" className="hidden h-7 w-auto xl:block" />
    </span>
  );
}
