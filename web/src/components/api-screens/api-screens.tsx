"use client";

import { type ReactNode, useEffect, useState } from "react";
import Link from "next/link";
import type { AgentOrderState } from "@/api/activity";
import type { AgentDetail, ApprovalDetail, ApprovalState, BoundOrder, MandateSummary } from "@/api/read-models";
import { deadlineIso } from "@/api/read-models";
import { ModeBadge } from "@/components/domain/mode";
import { DECISION_KEY, KEY } from "@/components/kumo/key";
import { PAGE_GRID, Panel, Section, WorkspaceGate } from "@/components/screens/common";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { type ShownChallenge, approvalConfirmCommand } from "@/api/step-up";
import { HostConfirm } from "./host-confirm";
import { APPROVER_SENTENCE, type DeferredChecks, deferredCheckLabel, deferredChecks } from "@/lib/deferred-checks";
import { clock, percent, price, quantity, usd } from "@/lib/format";
import { APPROVE_REFUSED, FINAL_UNDELIVERED, refusedText, undeliveredText } from "@/lib/grant-refusals";
import { type ActivityAnswer, type ApprovalRecord, type ApprovalResponse, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";
import { AgentHolds } from "./holds";
import { AgentTheses } from "./theses";

/**
 * The screens over the workspace API's read models (DEC-1096): Home, the agents, one agent, the
 * approvals and one request, each showing exactly what the API serves and "not reported" for the
 * rest. Nothing is computed in a missing member's place, and no P&L is shown: the API serves none.
 */

/** The fixture screen over the fixtures, the API's own over the API. */
export function BySource({ fixtures, api }: { fixtures: ReactNode; api: ReactNode }) {
  const { source } = useRuntime();
  return <>{source === "api" ? api : fixtures}</>;
}

const NOT_REPORTED = "text-muted-foreground";

function NotReported({ children }: { children: ReactNode }) {
  return <span className={NOT_REPORTED}>{children}</span>;
}

/** The read models, once served; the gate shows loading, unreachable, signed out or an unread membership instead. */
function useView() {
  return useRuntime().view;
}

function nameOf(agent: Pick<AgentDetail, "label"> | undefined): string | null {
  return agent?.label ?? null;
}

function orderLine(order: BoundOrder): string {
  return `buy ${quantity(order.qty)} ${order.instrument} at a limit of ${price(order.limit)}`;
}

function AgentName({ agent }: { agent: AgentDetail }) {
  const name = nameOf(agent);
  return name ? <>{name}</> : <NotReported>Name not reported</NotReported>;
}

function Mode({ agent }: { agent: AgentDetail }) {
  return agent.mode ? <ModeBadge mode={agent.mode} /> : <NotReported>Mode not reported</NotReported>;
}

function AgentRows({ agents }: { agents: AgentDetail[] }) {
  if (agents.length === 0) return <p className="text-muted-foreground">Your deployment reports no agents.</p>;
  return (
    <ul className="grid">
      {agents.map((agent) => (
        <li key={agent.agent_id} data-slot="api-agent" className="flex flex-wrap items-center gap-x-4 gap-y-1.5 border-b border-border/70 py-3.5 last:border-b-0">
          <Link href={`/agents/${agent.agent_id}`} className="text-h3 underline-offset-4 hover:underline">
            <AgentName agent={agent} />
          </Link>
          <Mode agent={agent} />
          <EnvironmentBadge environment={agent.environment} />
          <span className="text-sm text-muted-foreground">
            {agent.pending_approvals === 0 ? "No request waiting" : agent.pending_approvals === 1 ? "1 request waiting" : `${agent.pending_approvals} requests waiting`}
          </span>
        </li>
      ))}
    </ul>
  );
}

function requestText(approval: ApprovalDetail, agents: AgentDetail[]): string {
  const name = nameOf(agents.find((a) => a.agent_id === approval.agent_id)) ?? "An agent whose name is not reported";
  return `${name} asks to ${orderLine(approval.order)}`;
}

const byDeadline = (a: ApprovalDetail, b: ApprovalDetail) => a.deadline - b.deadline;

function RequestLinks({ approvals, agents }: { approvals: ApprovalDetail[]; agents: AgentDetail[] }) {
  return (
    <ul className="grid">
      {approvals.map((approval) => (
        <li key={approval.approval_id} className="grid gap-1 border-b border-border/70 py-3.5 last:border-b-0">
          <Link href={`/approvals/${approval.approval_id}`} className="font-medium underline-offset-4 hover:underline">
            {requestText(approval, agents)}
          </Link>
          <span className="text-sm text-muted-foreground">Skipped at {clock(deadlineIso(approval.deadline))} unless you answer</span>
        </li>
      ))}
    </ul>
  );
}

export function ApiHome() {
  const view = useView();
  return (
    <WorkspaceGate api>
      {view ? (
        <div className={PAGE_GRID}>
          <div className="grid content-start gap-(--section-gap)">
            <h1 className="sr-only">Home</h1>
            <Section title="Needs you">
              {view.approvals.some((a) => a.state === "pending") ? (
                <RequestLinks approvals={view.approvals.filter((a) => a.state === "pending").sort(byDeadline)} agents={view.agents} />
              ) : (
                <p className="text-muted-foreground">Nothing needs you.</p>
              )}
            </Section>
            <Section title="Agents">
              <AgentRows agents={view.agents} />
            </Section>
          </div>
        </div>
      ) : null}
    </WorkspaceGate>
  );
}

export function ApiAgents() {
  const view = useView();
  return (
    <WorkspaceGate api>
      {view ? (
        <div className="grid content-start gap-(--section-gap)">
          <h1 className="text-h1">Agents</h1>
          <Section title="Agents" id="api-agents">
            <AgentRows agents={view.agents} />
          </Section>
        </div>
      ) : null}
    </WorkspaceGate>
  );
}

const GOAL: Record<MandateSummary["goal"], string> = { continuous: "Trade continuously", accumulate: "Accumulate", profit_stop: "Stop at a profit level" };

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="grid gap-1">
      <dt className="text-sm text-muted-foreground">{term}</dt>
      <dd className="font-mono text-figure tabular">{children}</dd>
    </div>
  );
}

function MandateFacts({ mandate }: { mandate: MandateSummary }) {
  return (
    <dl className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-3">
      <Fact term="Allocation">{usd(mandate.allocation_usd)}</Fact>
      <Fact term="Most it may lose">{percent(mandate.max_loss_from_allocation)}</Fact>
      <Fact term="Goal">{GOAL[mandate.goal]}</Fact>
      <Fact term="Ends">{mandate.goal_end_date ?? "No end date"}</Fact>
      <Fact term="Asset classes">{mandate.asset_classes.map((c) => (c === "crypto" ? "Crypto" : "US equities")).join(", ")}</Fact>
      <Fact term="Largest position">{usd(mandate.max_position_usd)}</Fact>
      <Fact term="Largest order">{usd(mandate.max_order_usd)}</Fact>
      <Fact term="Orders a day">{mandate.max_orders_per_day}</Fact>
      <Fact term="Daily loss limit">{percent(mandate.max_daily_loss)}</Fact>
      <Fact term="Drawdown limit">{percent(mandate.max_drawdown)}</Fact>
    </dl>
  );
}

function Positions({ agent }: { agent: AgentDetail }) {
  if (agent.account_positions === null) return <p className={NOT_REPORTED}>Account positions not reported</p>;
  if (agent.account_positions.length === 0) return <p className="text-muted-foreground">The account held nothing at its last reconciliation.</p>;
  return (
    <table className="w-full text-left text-sm">
      <thead className="text-muted-foreground">
        <tr>
          <th className="py-2 font-normal">Instrument</th>
          <th className="py-2 font-normal">Held</th>
          <th className="py-2 font-normal">At the broker</th>
          <th className="py-2 font-normal">As of</th>
        </tr>
      </thead>
      <tbody>
        {agent.account_positions.map((p) => (
          <tr key={p.instrument} className="border-t border-border/70">
            <td className="py-2.5 font-medium">{p.instrument}</td>
            <td className="py-2.5 font-mono tabular">{quantity(p.qty)}</td>
            <td className="py-2.5 font-mono tabular">
              {quantity(p.broker_qty)}
              {p.mismatch ? <span className="ms-2 font-sans text-foreground">Differs from the broker</span> : null}
            </td>
            <td className="py-2.5 text-muted-foreground">{clock(p.observed_at)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function PauseControl({ agent }: { agent: AgentDetail }) {
  const { send, commands } = useRuntime();
  const canPause = useCan("stop.pause");
  const [sent, setSent] = useState<string | null>(null);
  const command = commands.find((c) => c.id === sent);
  if (!canPause || agent.mode === "stopped" || agent.mode === "paused") return null;
  return (
    <div className="flex flex-wrap items-center gap-3">
      <button type="button" className={KEY} onClick={() => setSent(send("pause", agent.agent_id).id)}>
        Pause
      </button>
      {command ? (
        <span role="status" className="text-sm text-muted-foreground">
          {command.phase === "recorded" ? "Pause recorded in the journal." : command.phase === "sent" ? "Pause sent." : command.phase === "unknown" ? "The result is unknown; we are checking." : "Not recorded."}
        </span>
      ) : null}
    </div>
  );
}

export function ApiAgent({ agentId }: { agentId: string }) {
  const view = useView();
  const agent = view?.agents.find((a) => a.agent_id === agentId);
  return (
    <WorkspaceGate api>
      {view && !agent ? <h1 className="text-h1">Agent not found</h1> : null}
      {agent ? (
        <article className="grid content-start gap-(--section-gap)">
          <header className="grid gap-3">
            <div className="flex flex-wrap items-center gap-3">
              <Mode agent={agent} />
              <EnvironmentBadge environment={agent.environment} />
            </div>
            <h1 className="text-h1">
              <AgentName agent={agent} />
            </h1>
            <PauseControl agent={agent} />
            <OpeningsHeldNote held={agent.openings_held} />
          </header>
          <AgentHolds agentId={agent.agent_id} />
          <Section title="Mandate">{agent.mandate ? <MandateFacts mandate={agent.mandate} /> : <p className={NOT_REPORTED}>Mandate not reported</p>}</Section>
          <Section title="Account positions">
            <p className="text-sm text-muted-foreground">What the account held at its last reconciliation, whichever agent bought it.</p>
            <Positions agent={agent} />
          </Section>
          <AgentTheses agentId={agent.agent_id} />
          <AgentActivity agentId={agent.agent_id} />
          <Section title="Limit usage">
            <p className={NOT_REPORTED}>Not reported by your deployment yet.</p>
          </Section>
        </article>
      ) : null}
    </WorkspaceGate>
  );
}

/**
 * Why every opening of the agent's is held, in words (DEC-2143), so a held agent never reads as a
 * stalled one. Nothing is shown when no standing reason applies; exits always still run (`AGENTS.md`
 * rule 13).
 */
export function OpeningsHeldNote({ held }: { held: AgentDetail["openings_held"] }) {
  if (held === null) return null;
  const why =
    held.reason === "research_agent_traded"
      ? "Openings held: this research agent's account has traded, and a further opening waits for the multi-instrument risk state, which is not built yet."
      : held.reason === "account_traded"
        ? "Openings held: this account has traded, and your deployment does not yet read the account's risk for a new opening."
        : "Openings may be held; status unavailable.";
  return (
    <p className="rounded-xl bg-lapis-soft px-4 py-3 text-sm" role="status" data-slot="openings-held" data-reason={held.reason}>
      {why} Exits still run.
    </p>
  );
}

/** An order's journaled state in words; `null` is not reported, never assumed (DEC-2111), and unknown stays unknown (trading spec §5.7). */
const ORDER_STATE: Readonly<Record<AgentOrderState, string>> = {
  intent: "Recorded, not sent yet",
  submitting: "Sending",
  accepted: "Working",
  partially_filled: "Partly filled",
  pending_cancel: "Canceling",
  pending_replace: "Replacing",
  unknown: "Unknown: being checked with the broker",
  filled: "Filled",
  canceled: "Canceled",
  rejected: "Rejected",
  expired: "Expired",
  replaced: "Replaced",
  abandoned: "Abandoned",
};

/** How often the agent's activity is read again while its screen is open. */
const ACTIVITY_POLL_MS = 5000;

/**
 * The agent's orders and its own positions (§4.7 D3, DEC-2111), newest order first, read when the
 * screen opens and every few seconds after. A list that cannot be read says not reported, never
 * none, and a missing account says so: neither is an empty record.
 */
function AgentActivity({ agentId }: { agentId: string }) {
  const { activity } = useRuntime();
  const [answer, setAnswer] = useState<ActivityAnswer | null>(null);
  useEffect(() => {
    if (!activity) return;
    let live = true;
    const load = () => void activity(agentId).then((next) => live && setAnswer(next));
    load();
    const poll = window.setInterval(load, ACTIVITY_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [activity, agentId]);
  const orders = answer?.orders;
  const holdings = answer?.holdings;
  const journal = answer?.journal;
  return (
    <>
      <Section title="Orders">
        {!orders ? (
          <p className={NOT_REPORTED}>Reading orders.</p>
        ) : !orders.ok ? (
          <p className={NOT_REPORTED}>Orders not reported.</p>
        ) : orders.account_ref === null ? (
          <p className={NOT_REPORTED}>Orders not reported: the deployment found no account for this agent.</p>
        ) : orders.items.length === 0 ? (
          <p className="text-muted-foreground">No order yet.</p>
        ) : (
          <ul className="grid" aria-label="Orders">
            {[...orders.items].reverse().map((o) => (
              <li key={o.client_order_id} className="grid gap-1 border-b border-border/70 py-3.5 last:border-b-0" data-state={o.state ?? "not_reported"}>
                <span>
                  {o.side === "buy" ? "Buy" : "Sell"} {quantity(o.qty)} {o.instrument_id}
                  {o.limit_price !== null ? ` at a limit of ${price(o.limit_price)}` : ""}
                </span>
                <span className="text-sm text-muted-foreground">
                  {o.state === null ? "State not reported" : ORDER_STATE[o.state]}
                  {o.filled_qty !== null ? `, ${quantity(o.filled_qty)} filled` : ", filled quantity not reported"}, sent {clock(o.submitted_at)} UTC
                </span>
              </li>
            ))}
          </ul>
        )}
      </Section>
      <Section title="Journal">
        <p className="text-sm text-muted-foreground">The latest records on this agent&rsquo;s stream, oldest first.</p>
        {!journal ? (
          <p className={NOT_REPORTED}>Reading the journal.</p>
        ) : !journal.ok ? (
          <p className={NOT_REPORTED}>{journal.reason === "forbidden" ? "Not available to your role." : "Journal not reported."}</p>
        ) : journal.items.length === 0 ? (
          <p className="text-muted-foreground">Nothing recorded yet.</p>
        ) : (
          <ol className="grid" aria-label="Journal">
            {journal.items.map((e) => (
              <li key={e.event_id} className="flex justify-between gap-3 border-b border-border/70 py-2.5 text-sm last:border-b-0">
                <span>
                  <span className="text-muted-foreground tabular-nums">#{e.seq}</span> {e.event_type}
                </span>
                <span className="text-muted-foreground tabular-nums">{clock(e.recorded_at)} UTC</span>
              </li>
            ))}
          </ol>
        )}
      </Section>
      <Section title="This agent's positions">
        <p className="text-sm text-muted-foreground">What this agent&rsquo;s own fills hold, net, per instrument.</p>
        {!holdings ? (
          <p className={NOT_REPORTED}>Reading positions.</p>
        ) : !holdings.ok || holdings.account_ref === null ? (
          <p className={NOT_REPORTED}>Not reported.</p>
        ) : holdings.items.length === 0 ? (
          <p className="text-muted-foreground">This agent holds nothing.</p>
        ) : (
          <ul className="grid" aria-label="This agent's positions">
            {holdings.items.map((h) => (
              <li key={h.instrument_id} className="flex justify-between gap-3 border-b border-border/70 py-3 last:border-b-0">
                <span>{h.instrument_id}</span>
                <span className="tabular-nums">{quantity(h.filled_qty)}</span>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </>
  );
}

const ENDED: Record<Exclude<ApprovalState, "pending">, string> = {
  acted: "Approved",
  skipped: "Skipped",
  timed_out: "Skipped at the deadline",
  canceled: "Canceled",
};

export function ApiApprovals() {
  const view = useView();
  const waiting = view?.approvals.filter((a) => a.state === "pending").sort(byDeadline) ?? [];
  const resolved = view?.approvals.filter((a) => a.state !== "pending") ?? [];
  return (
    <WorkspaceGate api>
      {view ? (
        <div className="grid content-start gap-(--section-gap)">
          <h1 className="text-h1">Approvals</h1>
          <Section title="Waiting">{waiting.length > 0 ? <RequestLinks approvals={waiting} agents={view.agents} /> : <p className="text-muted-foreground">Nothing is waiting.</p>}</Section>
          <Section title="Resolved">
            {resolved.length > 0 ? (
              <ul className="grid">
                {resolved.map((approval) => (
                  <li key={approval.approval_id} className="grid gap-1 border-b border-border/70 py-3.5 last:border-b-0">
                    <Link href={`/approvals/${approval.approval_id}`} className="underline-offset-4 hover:underline">
                      {requestText(approval, view.agents)}
                    </Link>
                    <span className="text-sm text-muted-foreground">{ENDED[approval.state as Exclude<ApprovalState, "pending">]}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-muted-foreground">No request has ended yet.</p>
            )}
          </Section>
        </div>
      ) : null}
    </WorkspaceGate>
  );
}

function responseText(response: ApprovalResponse): string {
  switch (response.phase) {
    case "sent":
      return response.response === "approve" ? "Your approval was sent. Not recorded yet." : "Your skip was sent. Not recorded yet.";
    case "recorded":
      return response.response === "approve" ? "Recorded in the journal: you approved. The gate re-runs its checks before anything is sent." : "Recorded in the journal: you skipped. Nothing is sent.";
    case "decided":
      return "Approved: the gate re-ran its checks and allowed it.";
    case "refused":
      return refusedText(response.reason ?? null);
    case "undelivered":
      return undeliveredText(response.reason);
    case "unknown":
      return "The result is unknown; we are checking. Until the journal answers, treat the request as not approved.";
    default: {
      const unhandled: never = response.phase;
      throw new Error(`unhandled response phase ${String(unhandled)}`);
    }
  }
}

/** Why no code could be shown; nothing was asked of the host and nothing was approved. */
function noCode(reason: string): string {
  if (reason === "live_unavailable") return "No code: a host confirmation is for paper only. Nothing was approved.";
  if (reason === "forbidden" || reason === "role_forbidden") return "No code: your role cannot approve here. Nothing was approved.";
  return "No code: your deployment did not answer with one. Nothing was approved; try again.";
}

const DEFERRED_UNREADABLE = "This request names checks that have not been run yet, and they cannot be read. Approving is not offered: skip it, or let it time out.";

/** The lines the deferred checks add to what the approver is shown, in screen order (DEC-2515 item 5). */
function deferredLines(deferred: DeferredChecks): string[] {
  if (deferred.kind === "none") return [];
  if (deferred.kind === "unreadable") return [DEFERRED_UNREADABLE];
  return [APPROVER_SENTENCE, ...deferred.names.map(deferredCheckLabel)];
}

/**
 * The checks the dry run left to the binding gate, under the fixed sentence (mandate spec §6.4): what
 * has not happened yet, never a pass. A list it cannot read says so rather than show none.
 */
function DeferredChecksPanel({ deferred }: { deferred: Exclude<DeferredChecks, { kind: "none" }> }) {
  return (
    <section aria-label="Checks not run yet" data-slot="deferred-checks">
      <Panel>
        {deferred.kind === "unreadable" ? (
          <p className="text-pretty">{DEFERRED_UNREADABLE}</p>
        ) : (
          <div className="grid gap-2">
            <p className="text-pretty">{APPROVER_SENTENCE}</p>
            <ul className="list-disc ps-5">
              {deferred.names.map((name) => (
                <li key={name}>{deferredCheckLabel(name)}</li>
              ))}
            </ul>
          </div>
        )}
      </Panel>
    </section>
  );
}

type Asking = { phase: "asking" } | { phase: "shown"; shown: ShownChallenge; command: string } | { phase: "failed"; reason: string };

export function ApiApproval({ approvalId }: { approvalId: string }) {
  const view = useView();
  const { respond, responses, stepUp } = useRuntime();
  const [asking, setAsking] = useState<Asking | null>(null);
  const canRespond = useCan("approvals.respond");
  const approval = view?.approvals.find((a) => a.approval_id === approvalId);
  const agent = view?.agents.find((a) => a.agent_id === approval?.agent_id);
  const response = approval ? responses[approval.approval_id] : undefined;
  const refusedNow = response?.phase === "undelivered" && APPROVE_REFUSED.has(response.reason ?? "");
  const [approveClosed, setApproveClosed] = useState(false);
  if (refusedNow && !approveClosed) setApproveClosed(true);
  if (!approval) {
    return <WorkspaceGate api>{view ? <h1 className="text-h1">Request not found</h1> : null}</WorkspaceGate>;
  }
  const o = approval.order;
  const line = `Buy ${quantity(o.qty)} ${o.instrument} at a limit of ${price(o.limit)}`;
  const facts: Array<[string, string]> = [
    ["Order value", usd(o.order_usd)],
    ["Purpose", o.purpose === "open" ? "Opens a position" : "Adds to a position"],
    ["Time in force", o.tif ?? "Not reported"],
    ["Protection", o.protection ? `Stop at ${price(o.protection.stop)}${o.protection.take_profit ? `, take profit at ${price(o.protection.take_profit)}` : ""}` : "Not reported"],
    ["Deadline", clock(deadlineIso(approval.deadline))],
  ];
  const deferred = deferredChecks(approval.content, approval.schema_version, approval.decision_deferred_to_binding);
  const shown = [nameOf(agent) ?? "Name not reported", line, ...facts.map(([term, value]) => `${term}: ${value}`), ...deferredLines(deferred)];
  const environment = agent?.environment ?? "unknown";
  const record: ApprovalRecord = { screen: "D6", environment, shown, modelOutputExpanded: false };
  const skip = () => {
    setAsking(null);
    respond(approval.approval_id, "skip", record);
  };
  /** Approve asks for a code first (DEC-2104): one challenge per press, bound to this request's content hash. */
  const approve = () => {
    const digest = approval.content_hash;
    if (deferred.kind === "unreadable") return;
    if (!stepUp || !digest) return setAsking({ phase: "failed", reason: "no_step_up" });
    setAsking({ phase: "asking" });
    void stepUp({ kind: "approve", digest }).then((answer) => {
      if (!answer.ok) return setAsking({ phase: "failed", reason: answer.reason });
      const command = approvalConfirmCommand({ workspaceId: answer.workspaceId, principalId: answer.principalId, agentId: approval.agent_id, approvalId: approval.approval_id, shown: answer.shown });
      setAsking({ phase: "shown", shown: answer.shown, command });
    });
  };
  const submit = (shownChallenge: ShownChallenge) => respond(approval.approval_id, "approve", record, shownChallenge.challenge);
  const settled = response && (response.phase !== "undelivered" || FINAL_UNDELIVERED.has(response.reason ?? ""));
  const open = approval.state === "pending" && canRespond && !settled;
  const approveRefused = refusedNow || approveClosed;
  const approvable = deferred.kind !== "unreadable" && !approveRefused;
  return (
    <WorkspaceGate api>
      <article className="mx-auto grid w-full max-w-2xl grid-cols-1 gap-6" aria-labelledby="api-request-title">
        <header className="grid gap-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h1 id="api-request-title" className="text-sm font-medium text-muted-foreground">
              Approval request
            </h1>
            <EnvironmentBadge environment={environment} />
          </div>
          <p className="text-muted-foreground">{agent ? <AgentName agent={agent} /> : <NotReported>Name not reported</NotReported>}</p>
          <p className="text-h1 text-pretty">{line}</p>
        </header>
        <Panel well>
          <dl className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-3">
            {facts.map(([term, value]) => (
              <Fact key={term} term={term}>
                {value === "Not reported" ? <NotReported>{value}</NotReported> : value}
              </Fact>
            ))}
          </dl>
        </Panel>
        {deferred.kind !== "none" ? <DeferredChecksPanel deferred={deferred} /> : null}
        {approval.state !== "pending" ? <p className="text-h3">{ENDED[approval.state]}</p> : null}
        {response ? (
          <p role={response.phase === "refused" || (response.phase === "undelivered" && (FINAL_UNDELIVERED.has(response.reason ?? "") || APPROVE_REFUSED.has(response.reason ?? ""))) ? "alert" : "status"} data-phase={response.phase} className="text-sm">
            {responseText(response)}
          </p>
        ) : null}
        {asking?.phase === "asking" ? (
          <p role="status" className="text-sm">
            Asking your deployment for a code. Nothing is approved yet.
          </p>
        ) : null}
        {asking?.phase === "failed" ? (
          <p role="status" className="text-sm">
            {noCode(asking.reason)}
          </p>
        ) : null}
        {open && approvable && asking?.phase === "shown" && response?.phase !== "sent" ? (
          <HostConfirm key={asking.shown.challenge.challenge_id} shown={asking.shown} command={asking.command} submitLabel="Approve" onSubmit={() => submit(asking.shown)} onCancel={() => setAsking(null)} />
        ) : null}
        {open && (asking?.phase !== "shown" || approveRefused) && asking?.phase !== "asking" ? (
          <div className={cn("grid gap-3", approvable ? "grid-cols-2" : "grid-cols-1")}>
            {approvable ? (
              <button type="button" className={DECISION_KEY} onClick={approve}>
                Approve
              </button>
            ) : null}
            <button type="button" className={DECISION_KEY} onClick={skip}>
              Skip
            </button>
          </div>
        ) : null}
      </article>
    </WorkspaceGate>
  );
}
