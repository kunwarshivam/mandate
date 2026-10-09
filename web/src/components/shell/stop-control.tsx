"use client";

import { useEffect, useId, useMemo, useState } from "react";
import { usePathname } from "next/navigation";
import { StopOctagon } from "@/components/icon";
import { cn } from "@/lib/utils";
import type { Workspace } from "@/fixtures/types";
import { StopSheet } from "@/components/stop/stop-sheet";
import { attentionText, stopAttention } from "@/lib/attention";
import { useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { OPEN_STOP_EVENT, openStop } from "./open-stop";

export { OPEN_STOP_EVENT, openStop } from "./open-stop";

/** The agent a path is scoped to, if any. IDs are opaque and never shown as titles. */
export function agentIdFrom(pathname: string): string | null {
  const match = /^\/(?:agents|messages)\/(agt_[0-9A-Z]+)/.exec(pathname);
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
 * The one Stop sheet in the frame. Every Stop button, the command palette and an agent's own page
 * open it through `OPEN_STOP_EVENT`, so the dock and the tab bar, which are both in the document at
 * every width, never stack two sheets.
 */
export function StopSheetHost() {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  const allowed = useCan("stop.open");

  useEffect(() => {
    const onOpen = () => setOpen(true);
    window.addEventListener(OPEN_STOP_EVENT, onOpen);
    return () => window.removeEventListener(OPEN_STOP_EVENT, onOpen);
  }, []);

  if (!allowed) return null;
  return <StopSheet open={open} onOpenChange={setOpen} agentId={agentIdFrom(pathname)} />;
}

export type StopPlace = "dock" | "tab" | "inline";

/** Each place's box and pill. Both tones share them, so nothing moves when Stop turns loud. */
const PLACE: Record<StopPlace, { button: string; pill: string; icon: string }> = {
  dock: {
    button: "h-12.5 rounded-2xl",
    pill: "h-12.5 gap-1 rounded-2xl pr-5 pl-3",
    icon: "size-6",
  },
  tab: {
    button: "h-(--tab-bar) w-full min-w-0 place-content-center",
    pill: "h-11 min-w-16 gap-0.5 rounded-xl pr-3 pl-2",
    icon: "size-6",
  },
  inline: {
    button: "h-11 rounded-lg",
    pill: "h-11 gap-1 rounded-lg pr-4.5 pl-2.5 lg:h-10 lg:pr-4 lg:pl-2 lg:text-sm",
    icon: "size-6",
  },
};

/**
 * A Stop button, for a role that may stop, never disabled, and never waiting for the dashboard or
 * any model (brief §5, rule 13). It sits at the end of the dock and the phone's tab bar, the
 * frame's persistent controls, apart from the navigation (DEC-452). It is quiet until something
 * needs the owner (DEC-206): an ink outline, then the filled ink pill, the only filled thing on the
 * dock or the tab bar, while `stopAttention` gives a reason. The hit area is 44px tall or more.
 */
export function StopButton({ place, className }: { place: StopPlace; className?: string }) {
  const allowed = useCan("stop.open");
  const { ws } = useRuntime();
  const reasons = useStopAttention(ws);
  const loud = reasons.length > 0;
  const descriptionId = useId();
  const { button, pill, icon } = PLACE[place];

  if (!allowed) return null;
  return (
    <>
      <button
        type="button"
        onClick={openStop}
        data-slot="stop-control"
        data-place={place}
        data-tone={loud ? "loud" : "quiet"}
        aria-haspopup="dialog"
        aria-describedby={loud ? descriptionId : undefined}
        className={cn("press group inline-grid shrink-0 items-center outline-none", button, className)}
      >
        <span
          data-slot="stop-pill"
          className={cn(
            "inline-flex items-center justify-center border-2 border-ink font-semibold transition-colors duration-(--duration-hover) ease-(--ease-out) group-focus-visible:ring-3 group-focus-visible:ring-ring group-focus-visible:ring-offset-2 motion-reduce:transition-none",
            pill,
            loud ? "bg-ink text-ink-foreground group-hover:bg-ink/88" : "bg-card text-ink group-hover:bg-background",
          )}
        >
          <StopOctagon className={icon} aria-hidden />
          Stop
        </span>
      </button>
      {loud ? (
        <span id={descriptionId} hidden>
          {attentionText(reasons)}
        </span>
      ) : null}
    </>
  );
}

/** A Stop button with a sheet of its own, for a screen outside the frame and the design specimen. */
export function StopControl({ className }: { className?: string }) {
  return (
    <>
      <StopButton place="inline" className={className} />
      <StopSheetHost />
    </>
  );
}
