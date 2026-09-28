import type { ReactNode } from "react";
import Link from "next/link";
import { cn } from "@/lib/utils";

/**
 * The only crimson in the product. Kumo's destructive variants are lint-banned; a kill switch is
 * this component and nothing else. It takes no `disabled` and no `loading`: progress is reported as
 * status text beside it, and the switch stays pressable in every state. With `href` it is the Stop
 * sheet's way into a record screen, where the kill switch itself is confirmed.
 */
export function KillSwitchButton({
  title,
  children,
  onClick,
  href,
  appearance = "filled",
}: {
  title: string;
  children?: ReactNode;
  onClick?: () => void;
  href?: string;
  appearance?: "filled" | "outline";
}) {
  const className = cn(
    "press grid min-h-11 w-full gap-1 rounded-xl px-4 py-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2",
    appearance === "filled" ? "bg-crimson text-crimson-foreground hover:bg-crimson/90" : "border-2 border-crimson bg-card text-foreground hover:bg-background",
  );
  const body = (
    <>
      <span className="text-base font-semibold">{title}</span>
      {children ? <span className={cn("text-sm", appearance === "filled" ? "" : "text-muted-foreground")}>{children}</span> : null}
    </>
  );
  const tone = appearance === "filled" ? "kill" : "kill-outline";
  return href ? (
    <Link href={href} data-slot="kill-switch" data-tone={tone} onClick={onClick} className={className}>
      {body}
    </Link>
  ) : (
    <button type="button" data-slot="kill-switch" data-tone={tone} onClick={onClick} className={className}>
      {body}
    </button>
  );
}
