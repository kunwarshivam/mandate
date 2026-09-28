"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import { Octagon } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import { StopSheet } from "@/components/stop/stop-sheet";
import { useCan } from "@/lib/roles";

export const OPEN_STOP_EVENT = "owlhead:open-stop";

/** The agent a path is scoped to, if any. IDs are opaque and never shown as titles. */
export function agentIdFrom(pathname: string): string | null {
  const match = /^\/agents\/(agt_[0-9A-Z]+)/.exec(pathname);
  return match ? match[1] : null;
}

/**
 * The Stop control is always rendered for a role that may stop, and never disabled. It does not
 * wait for the dashboard, the sidebar, or any model to load (brief §5, rule 13). Ink, because ink
 * is what a stopped agent looks like, and the one filled control in the header, so it is always the
 * heaviest thing there. The command palette opens it through `OPEN_STOP_EVENT`.
 */
export function StopControl({ compact = false, className }: { compact?: boolean; className?: string }) {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  const allowed = useCan("stop.open");

  useEffect(() => {
    const onOpen = () => setOpen(true);
    window.addEventListener(OPEN_STOP_EVENT, onOpen);
    return () => window.removeEventListener(OPEN_STOP_EVENT, onOpen);
  }, []);

  if (!allowed) return null;
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        data-slot="stop-control"
        aria-haspopup="dialog"
        className={cn(
          "press inline-flex h-11 shrink-0 items-center gap-2 rounded-full bg-ink pr-5 pl-4 font-semibold text-ink-foreground outline-none hover:bg-ink/88 focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 data-[compact=true]:px-3.5",
          className,
        )}
        data-compact={compact}
      >
        <Octagon className="size-5" weight="fill" aria-hidden />
        Stop
      </button>
      <StopSheet open={open} onOpenChange={setOpen} agentId={agentIdFrom(pathname)} />
    </>
  );
}
