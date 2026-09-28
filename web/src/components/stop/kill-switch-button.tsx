import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * The only crimson in the product. Kumo's destructive variants are lint-banned; a kill switch is
 * this component and nothing else. It takes no `disabled` and no `loading`: progress is reported as
 * status text beside it, and the switch stays pressable in every state.
 */
export function KillSwitchButton({
  title,
  children,
  onClick,
  appearance = "filled",
}: {
  title: string;
  children?: ReactNode;
  onClick: () => void;
  appearance?: "filled" | "outline";
}) {
  return (
    <button
      type="button"
      data-slot="kill-switch"
      data-tone={appearance === "filled" ? "kill" : "kill-outline"}
      onClick={onClick}
      className={cn(
        "press grid min-h-11 w-full gap-1 px-4 py-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2",
        appearance === "filled" ? "bg-crimson text-crimson-foreground hover:bg-crimson/90" : "border-2 border-crimson bg-card text-foreground hover:bg-muted",
      )}
    >
      <span className="text-base font-bold">{title}</span>
      {children ? <span className={cn("text-sm", appearance === "filled" ? "" : "text-muted-foreground")}>{children}</span> : null}
    </button>
  );
}
