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
      <PhaseLine command={command} at={clock(command.recordedAt ?? now)} recorded={recordedLine(command.kind, label)} />
    </p>
  );
}

/** Why a resume or Stop was not sent, in words: the passkey check's refusals, never their codes (DEC-2140). */
const PASSKEY_REFUSED: Readonly<Record<string, string>> = {
  passkey_unavailable: "this browser can't use a passkey",
  no_passkey: "you have no passkey on your deployment yet. Add one in Settings",
  passkey_cooling_off: "your passkey is new, and confirms only once its first 24 hours have passed",
  passkey_canceled: "the passkey check was canceled or timed out",
  passkey_failed: "your passkey's answer could not be read",
  control_moved: "something else was recorded while your passkey was asked, so its check no longer matches. Try again",
};

function PhaseLine({ command, at, recorded }: { command: Command; at: string; recorded: string }) {
  const { phase } = command;
  switch (phase) {
    case "sent":
      return command.awaitingPasskey ? "Waiting for your passkey. Nothing is sent until your browser's passkey check is done." : "Sent; waiting for the runtime to record it.";
    case "recorded":
      if (command.settled === "refused" || command.settled === "ended") return `Recorded at ${at}, then refused by the runtime: nothing changed from this request. The journal says why.`;
      if (command.awaitsRuntime && command.settled === "unconfirmed") return `Recorded at ${at}. The runtime has not confirmed it yet; we are checking. Nothing on screen changes until it does.`;
      if (command.awaitsRuntime && command.settled !== "applied") return `Recorded at ${at}. Waiting for the runtime to apply it.`;
      return `Recorded at ${at}. ${recorded}`;
    case "undelivered":
      if (command.reason && Object.hasOwn(PASSKEY_REFUSED, command.reason)) return `Not sent: ${PASSKEY_REFUSED[command.reason]}. Nothing changed. The kill switch still works.`;
      if (command.reason && command.reason !== "network") return "Not sent: your deployment refused it, so nothing changed from this request. The kill switch still works.";
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
        <p>{ws.health.deployment.as_of === null ? "It has not answered." : `It has not answered since ${clock(ws.health.deployment.as_of)}.`} A request from here cannot be delivered, and the screen will say so rather than show it as done.</p>
        <p>
          To stop trading now, go to the broker directly: sign in to your Alpaca paper dashboard, cancel open orders, and close positions there. Protective orders already
          resting at the broker stay in place until you cancel them.
        </p>
      </div>
    </div>
  );
}
