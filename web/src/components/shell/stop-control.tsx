"use client";

import { useEffect, useId, useMemo, useState } from "react";
import { usePathname } from "next/navigation";
import { Octagon } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";
import type { Workspace } from "@/fixtures/types";
import { StopSheet } from "@/components/stop/stop-sheet";
import { attentionText, stopAttention } from "@/lib/attention";
import { useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";

export const OPEN_STOP_EVENT = "owlhead:open-stop";

/** The agent a path is scoped to, if any. IDs are opaque and never shown as titles. */
export function agentIdFrom(pathname: string): string | null {
  const match = /^\/agents\/(agt_[0-9A-Z]+)/.exec(pathname);
  return match ? match[1] : null;
}

/**
 * The reasons Stop stands out, from `stopAttention`. While the workspace loads nothing is known, so
 * the last known reasons hold, starting from none: Stop never flickers on a navigation.
 */
export function useStopAttention(ws: Workspace): string[] {
  const [known, setKnown] = useState<string[]>([]);
  const current = useMemo(() => (ws.status === "loading" ? null : stopAttention(ws)), [ws]);
  if (current && current.join("\n") !== known.join("\n")) setKnown(current);
  return current ?? known;
}

/**
 * The Stop control is always rendered for a role that may stop, and never disabled. It does not
 * wait for the dashboard, the sidebar, or any model to load (brief §5, rule 13). It is quiet until
 * something needs the owner (DEC-206): an ink outline on the header, then the filled ink pill,
 * the only filled thing in the header, while `stopAttention` gives a reason. Both tones share one
 * box, so nothing moves when it turns, and the hit area is 44px tall at every width. The command
 * palette opens it through `OPEN_STOP_EVENT`.
 */
export function StopControl({ className }: { className?: string }) {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  const allowed = useCan("stop.open");
  const { ws } = useRuntime();
  const reasons = useStopAttention(ws);
  const loud = reasons.length > 0;
  const descriptionId = useId();

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
        data-tone={loud ? "loud" : "quiet"}
        aria-haspopup="dialog"
        aria-describedby={loud ? descriptionId : undefined}
        className={cn("press group inline-flex h-11 shrink-0 items-center rounded-full outline-none", className)}
      >
        <span
          data-slot="stop-pill"
          className={cn(
            "inline-flex h-11 items-center gap-2 rounded-full border-2 border-ink pr-4.5 pl-3.5 font-semibold transition-colors duration-(--duration-hover) ease-(--ease-out) group-focus-visible:ring-3 group-focus-visible:ring-ring group-focus-visible:ring-offset-2 motion-reduce:transition-none lg:h-9 lg:gap-1.5 lg:pr-4 lg:pl-3 lg:text-sm",
            loud ? "bg-ink text-ink-foreground group-hover:bg-ink/88" : "bg-card text-ink group-hover:bg-background",
          )}
        >
          <Octagon className="size-5 lg:size-4" weight="fill" aria-hidden />
          Stop
        </span>
      </button>
      {loud ? (
        <span id={descriptionId} hidden>
          {attentionText(reasons)}
        </span>
      ) : null}
      <StopSheet open={open} onOpenChange={setOpen} agentId={agentIdFrom(pathname)} />
    </>
  );
}
