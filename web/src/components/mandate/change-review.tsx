"use client";

import { useId, useState } from "react";
import Link from "next/link";
import { KEY } from "@/components/kumo/key";
import { ChangeRows, ClassTag } from "@/components/screens/mandate-versions";
import { StepUpDialog } from "@/components/stop/step-up-dialog";
import type { Agent, MandateChange } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { dec, sub } from "@/lib/decimal";
import { clock, price, quantity, usd, zoneLabel } from "@/lib/format";
import { CHANGE_CLASS_LABEL } from "@/lib/labels";
import { type Effects, INDEPENDENT_APPROVAL_RULE, type Origin, type Proposal, changeWords, effectsOf } from "@/lib/mandate-change";
import { type MandateChangeRequest, useRuntime } from "@/lib/mock-runtime";
import { RESTRICTIONS } from "@/lib/restrictions";
import { useCan } from "@/lib/roles";
import { agentHref } from "@/lib/screens";
import { cn } from "@/lib/utils";

/** How the version applies and what it does besides its own fields (mandate spec §2.2, §5.1), in the order the review shows them. */
export function applyLines(agent: Agent, p: Proposal, e: Effects): string[] {
  const lines: string[] = [];
  if (p.refusals.some((r) => r.rule === INDEPENDENT_APPROVAL_RULE)) {
    lines.push("This can't be confirmed here, for the reason below.");
  } else if (p.classification === "risk_increasing") {
    lines.push("This raises risk, so it takes your passkey. It applies at the agent's next check with no order in an unknown state.");
    if (e.unknownOrder) lines.push("An order is in an unknown state now, so it waits until the broker answers for that order.");
  } else {
    lines.push("This applies when you confirm it. No passkey needed.");
  }
  const capital = p.changes.find((c) => c.path === "/capital/allocation_usd");
  if (capital) {
    const equity = dec(agent.state.equity);
    const after = sub(equity, sub(dec(String(capital.from)), dec(String(capital.to))));
    lines.push(`The agent's equity moves with its capital, from ${usd(equity)} to ${usd(after)}. Its drawdown and loss so far stay the same share of it.`);
  }
  if (e.approvalsWaiting > 0) {
    lines.push(`${e.approvalsWaiting === 1 ? "1 request waiting for you is" : `${e.approvalsWaiting} requests waiting for you are`} canceled when it applies; anything still wanted is proposed again under it.`);
  }
  for (const o of e.ordersCanceled) {
    lines.push(`The resting buy of ${quantity(o.qty)} ${o.instrument.symbol} at ${price(o.limit_price ?? "0")} is canceled when it applies: it is larger than the new limits allow.`);
  }
  if (e.latched.length > 0) {
    lines.push(`${e.latched.map((code) => RESTRICTIONS[code].label).join("; ")}: a limit already reached stays in force. A new version never clears it.`);
  }
  if (e.pendingNumber !== null) lines.push(`Version ${e.pendingNumber} is waiting for a safe point. If you confirm this one, version ${e.pendingNumber} never applies.`);
  return lines;
}

function titleFor(agent: Agent): string {
  return `Change ${agent.label}'s mandate`;
}

const classLine = (c: MandateChange) => `${changeWords(c)} (${CHANGE_CLASS_LABEL[c.classification]})`;

/** Every line the review shows, in order, as the journal keeps it with `MandateConfirmed` (brief §4.1, mandate §10). */
export function reviewLines(agent: Agent, p: Proposal, origin: Origin, e: Effects): string[] {
  return [
    titleFor(agent),
    ...(p.classification ? [CHANGE_CLASS_LABEL[p.classification]] : []),
    ...(origin.kind === "message" ? [`You said “${origin.quote}”`] : []),
    ...p.changes.map(classLine),
    ...applyLines(agent, p, e),
    `Version ${p.number}: ${p.version}`,
  ];
}

/** The one action the passkey authorizes, in words. */
export function changeAction(agent: Agent, p: Proposal): string {
  return `Change ${agent.label}'s mandate: ${p.changes.map(changeWords).join("; ")}.`;
}

function Progress({ request, agentId }: { request: MandateChangeRequest; agentId: string }) {
  const at = (iso: string) => `${clock(iso)} ${zoneLabel(iso)}`;
  const versions = (
    <Link href={agentHref(agentId, "mandate/versions")} className="w-fit font-medium text-lapis underline-offset-4 hover:underline">
      See it in Versions
    </Link>
  );
  switch (request.phase) {
    case "sent":
      return <p data-phase="sent">Sent at {at(request.sentAt)}. Nothing changes until the runtime records it.</p>;
    case "waiting":
      return (
        <div data-phase="waiting" className="grid gap-1">
          <p>
            Recorded at {at(request.recordedAt ?? request.sentAt)} as version {request.number}. It applies at the agent&apos;s next check with no order in an unknown state; until
            then the version before it stays in effect.
          </p>
          {versions}
        </div>
      );
    case "applied":
      return (
        <div data-phase="applied" className="grid gap-1">
          <p className="font-semibold">
            Version {request.number} applied at {at(request.appliedAt ?? request.sentAt)}.
          </p>
          {versions}
        </div>
      );
    case "rejected":
      return (
        <div data-phase="rejected" role="alert" className="grid gap-1">
          <p className="font-semibold">Not applied. The mandate is as it was.</p>
          <p className="text-pretty">{request.reason}</p>
        </div>
      );
    case "undelivered":
      return (
        <p data-phase="undelivered" role="alert" className="text-pretty">
          Not delivered: your deployment did not answer, so nothing changed. Try again once it is reachable.
        </p>
      );
    case "unknown":
      return (
        <p data-phase="unknown" className="text-pretty">
          The result is unknown; we are checking. Look in Versions before confirming it again, so the same change is not made twice.
        </p>
      );
    default: {
      const unhandled: never = request.phase;
      throw new Error(`unhandled phase ${String(unhandled)}`);
    }
  }
}

/**
 * A6's review of one proposed version: every changed field with its classification, how it applies,
 * and one confirmation, with a passkey when it raises risk (§9.2). It is the same in the Edit form
 * and in a message. What it shows when the owner confirms is what the journal keeps, and from then
 * on the card shows that version, whatever the form or the thread does next.
 */
export function ChangeReview({
  proposal,
  origin,
  onKeep,
  onSent,
  fixHint,
}: {
  proposal: Proposal;
  origin: Origin;
  /** "Keep as is": the parent drops the proposal. Without it, the card offers only Confirm. */
  onKeep?: () => void;
  onSent?: (request: MandateChangeRequest) => void;
  /** What the owner does about a refusal: change a value above, or say a different one. */
  fixHint: string;
}) {
  const { ws, now, mandateChanges, changeMandate } = useRuntime();
  const mayChange = useCan("agents.deploy");
  const headingId = useId();
  const [sent, setSent] = useState<{ id: string; proposal: Proposal; applies: string[] } | null>(null);
  const [asking, setAsking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const p = sent?.proposal ?? proposal;
  const agent = findAgent(ws, p.agentId);
  const request = sent ? mandateChanges.find((c) => c.id === sent.id) : undefined;
  if (!agent) return null;
  const effects = effectsOf(ws, agent, p.mandate, now);
  const applies = sent?.applies ?? applyLines(agent, p, effects);
  const stale = !sent && agent.mandate_version !== p.previous;
  const inEffect = agent.versions.findIndex((v) => v.mandate_version === p.previous) + 1;

  const confirm = () => {
    const made = changeMandate(p, origin, { screen: "A6", environment: ws.environment, shown: reviewLines(agent, p, origin, effects) });
    setSent({ id: made.id, proposal: p, applies });
    setNotice(null);
    onSent?.(made);
  };

  return (
    <section aria-labelledby={headingId} data-slot="change-review" data-classification={p.classification ?? undefined} className="grid max-w-2xl gap-4 rounded-2xl border border-border bg-card p-4 sm:p-5">
      <header className="grid gap-1">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <h3 id={headingId} className="text-h3">
            {titleFor(agent)}
          </h3>
          {p.classification ? <ClassTag value={p.classification} /> : null}
        </div>
        <p className="text-caption text-muted-foreground">
          Version {p.number}, diffed against version {inEffect}. Hash <span className="font-mono">{p.version.slice(7, 19)}</span>
        </p>
      </header>

      <ChangeRows changes={p.changes} />

      <ul data-slot="change-applies" className="grid max-w-measure gap-1.5 text-sm text-pretty">
        {applies.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>

      {p.refusals.length > 0 ? (
        <div data-slot="change-refusals" className="grid gap-1.5 rounded-xl bg-background px-4 py-3 text-sm">
          <p className="font-semibold">This can&apos;t be confirmed:</p>
          <ul className="grid gap-1 text-pretty">
            {p.refusals.map((r) => (
              <li key={r.text} data-rule={r.rule}>
                {r.text}
              </li>
            ))}
          </ul>
          <p className="text-muted-foreground">{fixHint}</p>
        </div>
      ) : null}

      {request || sent ? null : stale ? (
        <p className="text-sm text-pretty" data-slot="change-stale">
          Another version applied after this was shown, so it no longer diffs against the mandate in effect. Ask again to see the change against it.
        </p>
      ) : !mayChange ? (
        <p className="text-sm">Only the workspace owner can change a mandate.</p>
      ) : p.refusals.length === 0 ? (
        <div className="flex flex-wrap gap-2">
          <button type="button" className={KEY} onClick={() => (p.stepUp ? setAsking(true) : confirm())}>
            {p.stepUp ? "Confirm with passkey" : "Confirm change"}
          </button>
          {onKeep ? (
            <button type="button" className={KEY} onClick={onKeep}>
              Keep as is
            </button>
          ) : null}
        </div>
      ) : onKeep ? (
        <div>
          <button type="button" className={KEY} onClick={onKeep}>
            Keep as is
          </button>
        </div>
      ) : null}

      <div role="status" aria-live="polite" className={cn("grid gap-2 text-sm", request ? "border-l-2 border-border pl-4" : null)}>
        {notice ? <p>{notice}</p> : null}
        {request ? <Progress request={request} agentId={agent.agent_id} /> : null}
      </div>

      <StepUpDialog
        open={asking}
        action={changeAction(agent, p)}
        onVerified={() => {
          setAsking(false);
          confirm();
        }}
        onCancel={() => {
          setAsking(false);
          setNotice("Passkey check canceled. Nothing was confirmed or sent.");
        }}
        onFailed={() => {
          setAsking(false);
          setNotice("Passkey check failed. Nothing was confirmed or sent.");
        }}
      />
    </section>
  );
}
