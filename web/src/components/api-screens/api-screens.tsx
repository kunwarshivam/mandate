"use client";

import { type ReactNode, useState } from "react";
import Link from "next/link";
import type { AgentDetail, ApprovalDetail, ApprovalState, BoundOrder, MandateSummary } from "@/api/read-models";
import { deadlineIso } from "@/api/read-models";
import { ModeBadge } from "@/components/domain/mode";
import { DECISION_KEY, KEY } from "@/components/kumo/key";
import { PAGE_GRID, Panel, Section, WorkspaceGate } from "@/components/screens/common";
import { EnvironmentBadge } from "@/components/shell/environment-badge";
import { clock, percent, price, quantity, usd } from "@/lib/format";
import { type ApprovalResponse, useRuntime } from "@/lib/mock-runtime";
import { useCan } from "@/lib/roles";
import { cn } from "@/lib/utils";

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
          </header>
          <Section title="Mandate">{agent.mandate ? <MandateFacts mandate={agent.mandate} /> : <p className={NOT_REPORTED}>Mandate not reported</p>}</Section>
          <Section title="Account positions">
            <p className="text-sm text-muted-foreground">What the account held at its last reconciliation, whichever agent bought it.</p>
            <Positions agent={agent} />
          </Section>
          <Section title="Limit usage">
            <p className={NOT_REPORTED}>Not reported by your deployment yet.</p>
          </Section>
        </article>
      ) : null}
    </WorkspaceGate>
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
    case "undelivered":
      return response.reason === "step_up_required"
        ? "Not recorded: approving needs a passkey check your deployment did not receive, so nothing was approved."
        : "Not recorded: your deployment answered that nothing was recorded, so nothing was approved.";
    case "unknown":
      return "The result is unknown; we are checking. Until the journal answers, treat the request as not approved.";
    default: {
      const unhandled: never = response.phase;
      throw new Error(`unhandled response phase ${String(unhandled)}`);
    }
  }
}

export function ApiApproval({ approvalId }: { approvalId: string }) {
  const view = useView();
  const { respond, responses } = useRuntime();
  const canRespond = useCan("approvals.respond");
  const approval = view?.approvals.find((a) => a.approval_id === approvalId);
  const agent = view?.agents.find((a) => a.agent_id === approval?.agent_id);
  const response = approval ? responses[approval.approval_id] : undefined;
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
  const shown = [nameOf(agent) ?? "Name not reported", line, ...facts.map(([term, value]) => `${term}: ${value}`)];
  const environment = agent?.environment ?? "unknown";
  const answer = (choice: "approve" | "skip") => respond(approval.approval_id, choice, { screen: "D6", environment, shown, modelOutputExpanded: false });
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
        {approval.state !== "pending" ? <p className="text-h3">{ENDED[approval.state]}</p> : null}
        {response ? (
          <p role="status" data-phase={response.phase} className="text-sm">
            {responseText(response)}
          </p>
        ) : approval.state === "pending" && canRespond ? (
          <div className={cn("grid grid-cols-2 gap-3")}>
            <button type="button" className={DECISION_KEY} onClick={() => answer("approve")}>
              Approve
            </button>
            <button type="button" className={DECISION_KEY} onClick={() => answer("skip")}>
              Skip
            </button>
          </div>
        ) : null}
      </article>
    </WorkspaceGate>
  );
}
