import { cn } from "@/lib/utils";

/** A still block where content will be. No shimmer and no fake lines (DEC-200, flat colour only). */
export function Skeleton({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="skeleton-block" aria-hidden className={cn("bg-muted", className)} {...props} />;
}
