"use client";

import { useCallback, useEffect, useState } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { type Hold, holdWords } from "@/api/acknowledgments";
import { KEY } from "@/components/kumo/key";
import { Section } from "@/components/screens/common";
import { datedClock } from "@/lib/format";
import { type AcknowledgeOutcome, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";

/**
 * The holds that stand on an agent and its account, and the owner's acknowledgment of each (workspace
 * API spec §3.11 item 12, DEC-2144; trading spec §7.1 and §11): a tripped limit, an account
 * restriction, or a reconciliation hold, in words, never as a code. Acknowledging asks the passkey,
 * and only the executor's lift is announced as lifted; a recorded acknowledgment lifts nothing yet.
 * Over the fixtures there are no holds to read, so nothing renders.
 */
export function AgentHolds({ agentId }: { agentId: string }) {
  const { holds, now } = useRuntime();
  const [listed, setListed] = useState<{ ok: true; holds: Hold[] } | { ok: false } | null>(null);
  /** Set once an acknowledgment here was lifted, so its word stays after the list, read again, no longer holds it. */
  const [lifted, setLifted] = useState(false);
  const load = useCallback(async () => {
    if (!holds) return;
    const answer = await holds.list(agentId);
    setListed(answer.ok ? { ok: true, holds: answer.holds } : { ok: false });
  }, [holds, agentId]);
  useEffect(() => {
    if (!holds) return;
    let live = true;
    void holds.list(agentId).then((answer) => live && setListed(answer.ok ? { ok: true, holds: answer.holds } : { ok: false }));
    return () => {
      live = false;
    };
  }, [holds, agentId]);
  if (!holds || listed === null) return null;
  if (!listed.ok)
    return (
      <Section title="Holds to acknowledge">
        <p className="text-muted-foreground">Holds on this agent are not reported. If one stands, it is shown here once your deployment answers.</p>
      </Section>
    );
  if (listed.holds.length === 0 && !lifted) return null;
  return (
    <Section title="Holds to acknowledge">
      <div role="status" aria-live="polite">
        {lifted ? <p className="text-sm">Lifted: the executor journaled the lift. The agent goes back to what its other holds allow.</p> : null}
      </div>
      {listed.holds.length > 0 ? (
        <ul className="grid gap-4" aria-label="Holds on this agent">
          {listed.holds.map((h) => (
            <HoldItem
              key={`${h.subject}:${h.event}`}
              hold={h}
              now={now}
              onLifted={() => {
                setLifted(true);
                void load();
              }}
            />
          ))}
        </ul>
      ) : null}
    </Section>
  );
}

function HoldItem({ hold, now, onLifted }: { hold: Hold; now: string; onLifted: () => void }) {
  const { holds } = useRuntime();
  const mayAcknowledge = useCan("holds.acknowledge");
  const toasts = useKumoToastManager();
  const [state, setState] = useState<{ kind: "idle" } | { kind: "asking" } | { kind: "recorded" } | { kind: "done"; outcome: AcknowledgeOutcome }>({ kind: "idle" });
  const words = holdWords(hold);
  const acknowledge = async () => {
    if (!holds) return;
    setState({ kind: "asking" });
    const outcome = await holds.acknowledge(hold, () => setState({ kind: "recorded" }));
    setState({ kind: "done", outcome });
    if (outcome.phase === "applied") {
      toasts.add({ title: "Hold lifted", description: "Your acknowledgment, as the executor applied it." });
      onLifted();
    }
  };
  const busy = state.kind === "asking" || state.kind === "recorded";
  const settled = state.kind === "done" && state.outcome.phase === "applied";
  return (
    <li className="grid gap-2" data-slot="hold" data-subject={hold.subject}>
      <p className="font-medium text-pretty">{words.what}</p>
      <p className="text-sm text-pretty">{words.blocks}</p>
      <p className="text-sm text-muted-foreground">Since {datedClock(hold.raised_at, now, "UTC")} UTC.</p>
      {!mayAcknowledge ? (
        <p className="text-sm">Only the workspace owner or an operator can acknowledge a hold.</p>
      ) : hold.independent_required === true || hold.request !== null ? (
        <p className="text-sm text-pretty">Your workspace requires a second member to acknowledge it, so it is not offered here yet.</p>
      ) : settled ? null : (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} disabled={busy} onClick={() => void acknowledge()}>
            Acknowledge
          </Button>
          <p className="text-sm text-muted-foreground">Your browser asks for your passkey. Acknowledging resumes what this hold stops; it does not clear its cause.</p>
        </div>
      )}
      <div role="status" aria-live="polite">
        {state.kind === "asking" ? <p className="text-sm">Waiting for your passkey. Nothing is sent until your browser&apos;s passkey check is done.</p> : null}
        {state.kind === "recorded" || (state.kind === "done" && state.outcome.phase === "recorded") ? (
          <p className="text-sm">Recorded in the journal. The hold lifts only once the executor accepts it; this page shows it when it does.</p>
        ) : null}
        {state.kind === "done" ? <OutcomeWords outcome={state.outcome} /> : null}
      </div>
    </li>
  );
}

function OutcomeWords({ outcome }: { outcome: AcknowledgeOutcome }) {
  switch (outcome.phase) {
    case "applied":
    case "recorded":
      return null;
    case "refused":
      return (
        <p role="alert" className="text-sm text-pretty" data-reason={outcome.reason ?? ""}>
          Refused by the executor: {refusedWords(outcome.reason)} The hold stands.
        </p>
      );
    case "undelivered":
      return (
        <p role="alert" className="text-sm text-pretty" data-reason={outcome.reason}>
          {undeliveredWords(outcome.reason)}
        </p>
      );
    case "unknown":
      return <p className="text-sm">Whether your acknowledgment was recorded is not known yet. This list is read again; check it before acknowledging again.</p>;
    default: {
      const unhandled: never = outcome;
      throw new Error(`unhandled outcome ${JSON.stringify(unhandled)}`);
    }
  }
}

/** The executor's refusal of an acknowledgment copy (journal spec §9.28's four checks), in words. */
function refusedWords(reason: string | null): string {
  switch (reason) {
    case "trigger_unknown":
      return "it found no such hold.";
    case "trigger_superseded":
      return "the hold was raised again since, and needs its own acknowledgment.";
    case "not_independent":
      return "your workspace requires a second member to acknowledge it.";
    case "request_invalid":
      return "the request it answers could not be matched.";
    default:
      return reason?.startsWith("step_up") ? "it did not accept the passkey check." : "it gave a reason this page does not know.";
  }
}

/** Why nothing was recorded, in words. */
function undeliveredWords(reason: string): string {
  switch (reason) {
    case "acknowledgment_unavailable":
      return "Not recorded: this hold no longer stands, so there is nothing to acknowledge.";
    case "acknowledgment_request_invalid":
      return "Not recorded: the request it answers cannot be answered.";
    case "control_moved":
      return "Not sent: something was recorded for this workspace while your passkey was asked, so the check no longer matches. Try again.";
    case "step_up_mismatch":
      return "Not recorded: the passkey check no longer matches this workspace's journal. Try again.";
    case "step_up_required":
    case "step_up_missing":
    case "step_up_stale":
    case "step_up_method":
    case "step_up_reused":
      return "Not recorded: your deployment did not accept the passkey check.";
    case "passkey_unavailable":
      return "Not sent: this browser can't use a passkey.";
    case "no_passkey":
      return "Not sent: you have no passkey on your deployment yet. Add one in Settings, then try again.";
    case "passkey_cooling_off":
      return "Not sent: your passkey is new, and confirms only once its first 24 hours have passed.";
    case "passkey_canceled":
      return "Not sent: the passkey check was canceled or timed out.";
    case "passkey_failed":
      return "Not sent: your passkey's answer could not be read.";
    case "forbidden":
      return "Not recorded: your role may not acknowledge a hold.";
    case "live_unavailable":
      return "Not recorded: a real-money hold is acknowledged on the host, not here.";
    case "unreachable":
      return "Not delivered: your deployment did not answer.";
    default:
      return "Not recorded: your deployment refused it.";
  }
}
