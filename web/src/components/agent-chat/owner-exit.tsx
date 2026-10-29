"use client";

import { useEffect, useRef, useState } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import type { AgentHolding } from "@/api/activity";
import { isAssetId } from "@/api/commands";
import { quantity } from "@/lib/format";
import { type CommandRecord, type ExitOutcome, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";

/** Where the owner's exit from one instrument stands. */
type ExitStep = { step: "idle" } | { step: "confirming" } | { step: "sending" } | { step: "recorded" } | { step: "done"; outcome: ExitOutcome };

/** An exit's outcome in words: what the journal says, and that nothing was sold where nothing was recorded. */
function exitWords(state: ExitStep): string | null {
  switch (state.step) {
    case "sending":
      return "Sending your exit…";
    case "recorded":
      return "Recorded in the journal: your exit. Waiting for the agent's runtime to take it.";
    case "done":
      switch (state.outcome.phase) {
        case "applied":
          return "Taken by the agent's runtime. Its sell appears in Activity.";
        case "refused":
          return "Refused by the agent's runtime: nothing was sold for it.";
        case "ended":
          return "Ended without a sell for it.";
        case "recorded":
          return "Recorded in the journal: your exit. Its outcome is not reported yet.";
        case "undelivered":
          return "Not recorded: your deployment refused your exit. Nothing was sold.";
        case "unknown":
          return "The result is unknown; look at Activity before you exit again.";
      }
      return null;
    default:
      return null;
  }
}

const again = (state: ExitStep) => state.step === "idle" || (state.step === "done" && (state.outcome.phase === "undelivered" || state.outcome.phase === "refused"));

/**
 * The owner's exit from one instrument (§5.4, rule 13), deterministic and apart from the chat, so a
 * sale never waits on the model: asked first, then sent once through the exits route. The bid
 * confirmation is never sent, since no quote with its size and time is shown here, so an equity
 * sell outside the regular session waits for it.
 */
export function OwnerExit({ agentId, holding }: { agentId: string; holding: AgentHolding }) {
  const { ownerExit, ws, view } = useRuntime();
  const allowed = useCan("positions.exit");
  const [state, setState] = useState<ExitStep>({ step: "idle" });
  const sent = useRef(false);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const instrument = holding.instrument_id;
  if (!ownerExit || !allowed || holding.filled_qty.startsWith("-") || /^0(\.0*)?$/.test(holding.filled_qty)) return null;
  if (!isAssetId(instrument)) return <p className="text-sm text-muted-foreground">No exit here: this position names no asset id. The agent&apos;s kill switch still sells it.</p>;
  const lines = [
    `Sell the agent's whole position in ${instrument}: ${quantity(holding.filled_qty)} held by its fills.`,
    "Your exit is never denied for the agent's limits or conduct controls; only participation caps pace it.",
    "In the regular session it is sent at once. Outside it, an equity sell waits for the session.",
    "The journal records it with what this showed.",
  ];
  const sell = () => {
    if (sent.current) return;
    sent.current = true;
    const mode = view?.agents.find((a) => a.agent_id === agentId)?.mode ?? "not reported";
    const record: CommandRecord = { screen: "D10", environment: ws.environment, title: `Exit ${instrument}`, shown: lines, modes: [{ agent: agentId, badge: mode }] };
    const at = (next: ExitStep) => live.current && setState(next);
    at({ step: "sending" });
    void ownerExit(agentId, instrument, record, () => at({ step: "recorded" })).then((outcome) => {
      at({ step: "done", outcome });
      if (outcome.phase === "undelivered" || outcome.phase === "refused") sent.current = false;
    });
  };
  if (state.step === "confirming")
    return (
      <div role="group" aria-label={`Exit ${instrument}`} className="grid gap-2 text-sm">
        <ul className="grid gap-1">
          {lines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
        <div className="flex gap-2">
          <Button type="button" variant="primary" onClick={sell}>
            Sell it
          </Button>
          <Button type="button" variant="secondary" onClick={() => setState({ step: "idle" })}>
            Cancel
          </Button>
        </div>
      </div>
    );
  return (
    <div className="grid gap-1">
      {again(state) ? (
        <Button type="button" variant="secondary" aria-label={`Exit ${instrument}`} onClick={() => setState({ step: "confirming" })}>
          {state.step === "idle" ? "Exit" : "Exit again"}
        </Button>
      ) : null}
      <p role="status" className="text-sm">
        {exitWords(state)}
      </p>
    </div>
  );
}
