"use client";

import { Plug } from "pixelarticons/react/Plug.js";
import { clock } from "@/lib/format";
import { type Command, useRuntime } from "@/lib/mock-runtime";
import { commandTitle, recordedLine } from "./commands";

/** "Sent" until the runtime records it (rule 5); the recorded line only once it has. */
export function CommandEntry({ command, label }: { command: Command; label: string }) {
  const { now } = useRuntime();
  return (
    <p data-phase={command.phase} className="reveal rounded-xl bg-background px-4 py-3 text-sm">
      <span className="font-semibold">{commandTitle(command.kind, label)}.</span>{" "}
      <PhaseLine phase={command.phase} at={clock(command.recordedAt ?? now)} recorded={recordedLine(command.kind, label)} />
    </p>
  );
}

function PhaseLine({ phase, at, recorded }: { phase: Command["phase"]; at: string; recorded: string }) {
  switch (phase) {
    case "sent":
      return "Sent; waiting for the runtime to record it.";
    case "recorded":
      return `Recorded at ${at}. ${recorded}`;
    case "undelivered":
      return "Not delivered: your deployment did not answer, so nothing changed from this request. Use the broker directly, as described above.";
    case "unknown":
      return "The result is unknown; we are checking. Nothing on screen changes until the journal answers.";
    default: {
      const unhandled: never = phase;
      throw new Error(`unhandled phase ${String(unhandled)}`);
    }
  }
}

/** The deployment is not answering: say so, and give the route to the broker that still works. */
export function UnreachableAlert() {
  const { ws } = useRuntime();
  return (
    <div role="alert" className="grid gap-1.5 rounded-xl bg-background px-4 py-3 text-foreground" data-slot="unreachable">
      <p className="flex items-center gap-2 text-base font-semibold">
        <Plug className="size-6 shrink-0" aria-hidden />
        Cannot reach your deployment
      </p>
      <div className="grid gap-2 text-sm">
        <p>It has not answered since {clock(ws.health.deployment.as_of)}. A request from here cannot be delivered, and the screen will say so rather than show it as done.</p>
        <p>
          To stop trading now, go to the broker directly: sign in to your Alpaca paper dashboard, cancel open orders, and close positions there. Protective orders already
          resting at the broker stay in place until you cancel them.
        </p>
      </div>
    </div>
  );
}
