"use client";

import Link from "next/link";
import { ArrowLeft, ArrowRight, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Deadline } from "@/components/approvals/deadline";
import { Placeholder } from "@/components/domain/placeholders";
import { WorkspaceGate } from "@/components/screens/common";
import type { Agent, AgentMode } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { type Dec, dec, ratio } from "@/lib/decimal";
import { clock, price, quantity, usd } from "@/lib/format";
import { actionSentence, gateRule, verdictLabel } from "@/lib/gate-reasons";
import { MODE_LABEL, PURPOSE_LABEL, RISK_CAP_LABEL, RISK_FIGURE_LABEL } from "@/lib/labels";
import { agentLimits } from "@/lib/limits";
import { describeRestriction } from "@/lib/restrictions";
import { Figure, ResponseText, STAGES, Signed, outcomeOf, stageOf, stagger, useApproval, useDashboard } from "./shared";

/** A mode is a sign you read from across the room: running is quiet, anything else is a coloured field. */
const MODE_FIELD: Record<AgentMode, string> = {
  normal: "bg-muted text-foreground",
  exits_only: "bg-(--exits) text-(--exits-foreground)",
  paused: "bg-ink text-ink-foreground",
  stopped: "bg-ink text-ink-foreground",
};

/** A limit on the marigold mandate field: ink fills toward a lapis wall. */
function Gauge({ label, used, cap }: { label: string; used: Dec; cap: Dec }) {
  const share = Math.min(ratio(used, cap), 1);
  return (
    <div className="grid gap-1.5" data-slot="limit-rail">
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-sm font-bold">{label}</span>
        <span className="font-mono text-sm tabular">
          <Figure value={usd(used, 0)} /> <span className="text-(--field-muted)">of {usd(cap, 0)}</span>
        </span>
      </div>
      <div
        role="img"
        aria-label={`${label}: ${usd(used)} of a ${usd(cap)} limit`}
        className="relative h-3 bg-(--field-foreground)/15"
      >
        <div className="absolute inset-y-0 left-0 bg-(--field-foreground)" style={{ width: `${share * 100}%` }} />
        <div className="absolute inset-y-[-3px] right-0 w-1 bg-(--lapis)" />
      </div>
    </div>
  );
}

function Restrictions({ agent }: { agent: Agent }) {
  if (agent.restrictions.length === 0) return null;
  return (
    <ul className="grid gap-1 bg-notice px-4 py-3 text-sm text-foreground md:col-span-3">
      {agent.restrictions.map((r) => {
        const d = describeRestriction(r);
        return (
          <li key={`${r.code}-${r.symbol ?? ""}`}>
            <span className="font-bold">{d.title}.</span> Blocks {d.blocks.toLowerCase()}. Ends when{" "}
            {d.endsWhen.charAt(0).toLowerCase() + d.endsWhen.slice(1)}.
          </li>
        );
      })}
    </ul>
  );
}

function AgentBand({ agent, index }: { agent: Agent; index: number }) {
  const limits = agentLimits(agent);
  const rails = limits.rails.filter((r) => r.key === "gross" || r.key === "daily");
  return (
    <li className="d-reveal grid gap-1.5 md:grid-cols-[9.5rem_minmax(0,1fr)_minmax(0,1.15fr)]" style={stagger(index)}>
      <div data-mode={agent.mode} className={cn("flex items-end px-4 py-1.5 transition-colors duration-(--duration-hover) md:py-4", MODE_FIELD[agent.mode])}>
        <span className="font-display text-lg leading-[0.9] font-extrabold uppercase md:text-[1.75rem]">{MODE_LABEL[agent.mode]}</span>
      </div>
      <div className="grid content-start gap-2 bg-card px-4 py-4">
        <div className="flex items-baseline justify-between gap-3">
          <Link href={`/agents/${agent.agent_id}`} className="text-lg font-bold underline-offset-4 hover:underline">
            {agent.label}
          </Link>
          <span className="text-sm text-muted-foreground">{agent.mandate.name}</span>
        </div>
        <p className="font-display text-[2.75rem] leading-none font-bold">
          <Figure value={usd(agent.state.equity)} />
        </p>
        <p className="text-sm text-muted-foreground">equity, of {usd(agent.mandate.capital.allocation_usd, 0)} allocated</p>
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 text-sm">
          <span>Paper P&amp;L, simulated</span>
          <Signed value={agent.pnl_total} className="font-bold" />
          <Placeholder name="performance" className="rounded-none" />
        </div>
        <p className="text-sm">
          {agent.positions.length === 0
            ? "Flat, no positions."
            : `Holds ${agent.positions.map((p) => `${quantity(p.qty)} ${p.instrument.symbol}`).join(", ")}.`}
        </p>
      </div>
      <div className="grid content-start gap-4 bg-(--field) px-4 py-4 text-(--field-foreground)">
        <p className="font-display text-base font-extrabold uppercase">Your mandate</p>
        {rails.map((r) => (
          <Gauge key={r.key} label={r.label} used={r.used} cap={r.cap} />
        ))}
      </div>
      <Restrictions agent={agent} />
    </li>
  );
}

function Dashboard() {
  const { ws, now, open, running, positions } = useDashboard();
  if (ws.agents.length === 0) {
    return (
      <section aria-labelledby="p-empty" className="grid gap-4 bg-(--lapis) px-6 py-10 text-(--lapis-foreground)">
        <h1 id="p-empty" className="text-5xl font-extrabold uppercase">
          No agents yet
        </h1>
        <p className="max-w-lg text-(--lapis-muted)">An agent trades on paper within a mandate you describe and confirm, field by field.</p>
        <Link
          href="/agents/new"
          className="d-press inline-flex h-12 w-fit items-center gap-2 bg-(--field) px-5 text-lg font-bold text-(--field-foreground) hover:bg-(--field)/85"
        >
          Describe your first agent <ArrowRight aria-hidden className="size-5" />
        </Link>
      </section>
    );
  }
  return (
    <div className="grid gap-10">
      <div className="grid gap-1.5 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <header className="d-reveal grid content-start gap-5 bg-(--lapis) px-5 pt-6 pb-5 text-(--lapis-foreground)" style={stagger(0)}>
          <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
            <h1 className="text-[3.5rem] leading-[0.85] font-extrabold uppercase">Dashboard</h1>
            <p className="text-(--lapis-muted)">
              {ws.connection.broker}, <span className="font-mono tabular">{usd(ws.connection.account_equity)}</span> equity
            </p>
          </div>
          <table className="w-full border-collapse">
            <caption className="sr-only">Agents and their modes</caption>
            <thead className="sr-only">
              <tr>
                <th scope="col">Agent</th>
                <th scope="col">Mandate</th>
                <th scope="col">Mode</th>
              </tr>
            </thead>
            <tbody>
              {ws.agents.map((a) => (
                <tr key={a.agent_id} className="border-t border-(--lapis-muted)/40">
                  <th scope="row" className="py-2 pr-4 text-left font-display text-xl font-bold uppercase">
                    {a.label}
                  </th>
                  <td className="py-2 pr-4 text-(--lapis-muted)">{a.mandate.name}</td>
                  <td className={cn("py-2 text-right font-display text-xl font-extrabold uppercase", a.mode !== "normal" && "text-(--field)")}>{MODE_LABEL[a.mode]}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="text-(--lapis-muted)">
            {running} of {ws.agents.length} running
          </p>
        </header>
        <section aria-labelledby="p-waiting" className="d-reveal grid content-start gap-3 bg-card px-5 py-5" style={stagger(1)}>
          <h2 id="p-waiting" className="text-2xl font-extrabold uppercase">
            {open.length === 0 ? "Nothing waiting" : open.length === 1 ? "1 request waiting" : `${open.length} requests waiting`}
          </h2>
          {open.length === 0 ? (
            <p className="text-muted-foreground">Requests for your approval appear here, with their deadline.</p>
          ) : (
            <ul className="grid gap-3">
              {open.map((a) => {
                const agent = findAgent(ws, a.agent_id);
                return (
                  <li key={a.approval_id} className="grid gap-2 border-t-2 border-foreground pt-3">
                    <p className="text-lg font-bold">
                      {agent?.label ?? "An agent"} asks to buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at{" "}
                      <span className="font-mono tabular">{price(a.bound.limit)}</span>
                    </p>
                    <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
                    <Link
                      href={`/approvals/${a.approval_id}`}
                      className="d-press inline-flex h-11 w-fit items-center gap-2 bg-foreground px-4 font-bold text-background hover:bg-foreground/85"
                    >
                      Open request <ArrowRight aria-hidden className="size-4" />
                    </Link>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>

      <section aria-labelledby="p-agents" className="grid gap-3">
        <h2 id="p-agents" className="text-3xl font-extrabold uppercase">
          Agents
        </h2>
        <ul className="grid gap-6">
          {ws.agents.map((agent, i) => (
            <AgentBand key={agent.agent_id} agent={agent} index={i + 2} />
          ))}
        </ul>
      </section>

      <div className="grid gap-10 lg:grid-cols-[3fr_2fr]">
        <section aria-labelledby="p-decisions" className="grid content-start gap-3">
          <h2 id="p-decisions" className="text-3xl font-extrabold uppercase">
            Gate decisions
          </h2>
          {ws.decisions.length === 0 ? (
            <p className="border-t-2 border-foreground pt-3 text-muted-foreground">No decisions yet.</p>
          ) : (
            <ol className="border-t-2 border-foreground">
              {ws.decisions.slice(0, 6).map((d) => {
                const agent = findAgent(ws, d.agent_id);
                const rule = d.reason_code && agent ? gateRule(d.reason_code, agent.mandate) : null;
                const blocked = d.verdict !== "allow";
                return (
                  <li key={d.event_id} data-verdict={d.verdict} className="grid grid-cols-[3.25rem_minmax(0,1fr)] gap-3 border-b border-border py-3">
                    <time dateTime={d.at} className="font-mono text-sm text-muted-foreground tabular">
                      {clock(d.at).slice(0, 5)}
                    </time>
                    <div className="grid gap-1">
                      <p>
                        <span className={cn("mr-2 inline-block px-1.5 text-sm font-bold uppercase", blocked ? "bg-ink text-ink-foreground" : "bg-muted text-foreground")}>
                          {verdictLabel(d)}
                        </span>
                        {actionSentence(d.action)} <span className="text-muted-foreground">· {agent?.label}</span>
                      </p>
                      {rule ? <p className="text-sm text-muted-foreground">{rule}</p> : null}
                      {d.then ? <p className="text-sm text-muted-foreground">{d.then}</p> : null}
                    </div>
                  </li>
                );
              })}
            </ol>
          )}
        </section>

        <section aria-labelledby="p-positions" className="grid content-start gap-3">
          <h2 id="p-positions" className="text-3xl font-extrabold uppercase">
            Positions
          </h2>
          {positions.length === 0 ? (
            <p className="border-t-2 border-foreground pt-3 text-muted-foreground">No agent holds a position.</p>
          ) : (
            <table className="w-full border-collapse">
              <caption className="sr-only">Positions across agents</caption>
              <thead>
                <tr className="border-y-2 border-foreground text-left text-sm">
                  <th scope="col" className="py-2 pr-4 font-bold">Holding</th>
                  <th scope="col" className="py-2 pr-4 text-right font-bold">Value</th>
                  <th scope="col" className="py-2 text-right font-bold">Unrealized</th>
                </tr>
              </thead>
              <tbody>
                {positions.map(({ agent, p }) => (
                  <tr key={`${agent.agent_id}-${p.instrument.asset_id}`} className="border-b border-border">
                    <th scope="row" className="py-3 pr-4 text-left font-normal">
                      <span className="font-bold">
                        <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                      </span>
                      <span className="block text-sm text-muted-foreground">{agent.label}</span>
                    </th>
                    <td className="py-3 pr-4 text-right align-top font-mono tabular">
                      <Figure value={usd(p.market_value)} />
                    </td>
                    <td className="py-3 text-right align-top">
                      <Signed value={p.unrealized_pnl} word={false} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {ws.external_positions.length > 0 ? (
            <p className="text-sm text-muted-foreground">
              Also on the account, not managed by any agent: {ws.external_positions.map((e) => `${quantity(e.qty)} ${e.instrument.symbol}`).join(", ")}.
            </p>
          ) : null}
        </section>
      </div>
    </div>
  );
}

const CHOICE =
  "d-press h-14 w-full border-2 border-foreground bg-card text-lg font-bold text-foreground hover:bg-muted focus-visible:outline-2 focus-visible:outline-offset-2";

function Approval({ approvalId }: { approvalId: string }) {
  const view = useApproval(approvalId);
  if (!view) return <p className="text-muted-foreground">No approval request with this ID.</p>;
  const { approval, agent, response, respond, orderValue, open, now } = view;
  const b = approval.bound;
  const stage = stageOf(approval, response);
  const outcome = outcomeOf(approval);
  return (
    <article className="mx-auto grid w-full max-w-xl gap-1.5" aria-labelledby="p-request">
      <Link href="/approvals" className="mb-2 inline-flex w-fit items-center gap-1.5 font-bold underline-offset-4 hover:underline">
        <ArrowLeft className="size-4" aria-hidden /> Approvals
      </Link>
      <header className="d-reveal grid gap-2 bg-(--lapis) px-5 pt-6 pb-5 text-(--lapis-foreground)" style={stagger(0)}>
        <h1 id="p-request" className="text-[3.75rem] leading-[0.85] font-extrabold uppercase">
          Buy {quantity(b.qty)} {b.symbol}
        </h1>
        <p className="text-lg">
          at a limit of <span className="font-mono font-bold tabular">{price(b.limit)}</span>, <span className="font-mono tabular">{orderValue}</span> in all
        </p>
        <p className="text-(--lapis-muted)">
          {agent?.label ?? "An agent"} ({agent?.mandate.name ?? "unknown mandate"}) asks for your approval
        </p>
      </header>

      <dl className="d-reveal grid gap-3 bg-card px-5 py-4" style={stagger(1)}>
        <div className="grid gap-0.5">
          <dt className="text-sm text-muted-foreground">Why you are asked</dt>
          <dd className="text-lg font-bold">{approval.trigger}</dd>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-0.5">
            <dt className="text-sm text-muted-foreground">Purpose</dt>
            <dd>{PURPOSE_LABEL[b.purpose]}</dd>
          </div>
          <div className="grid gap-0.5">
            <dt className="text-sm text-muted-foreground">Mandate version</dt>
            <dd translate="no" className="font-mono text-sm">{b.mandate_version.slice(7, 19)}</dd>
          </div>
        </div>
        <div className="grid gap-0.5">
          <dt className="text-sm text-muted-foreground">Combined model score, not a probability of profit</dt>
          <dd className="font-mono tabular">{b.combined_score}</dd>
        </div>
      </dl>

      <section aria-labelledby="p-risk" className="d-reveal grid gap-4 bg-(--field) px-5 py-5 text-(--field-foreground)" style={stagger(2)}>
        <h2 id="p-risk" className="text-2xl font-extrabold uppercase">
          Against your mandate
        </h2>
        {approval.risk_impact.map((f) =>
          f.cap ? (
            <div key={f.field} className="grid gap-1">
              <Gauge label={RISK_FIGURE_LABEL[f.field]} used={dec(f.value)} cap={dec(f.cap)} />
              <span className="text-[0.8125rem] text-(--field-muted)">The {RISK_CAP_LABEL[f.field]}</span>
            </div>
          ) : (
            <p key={f.field} className="flex items-baseline justify-between gap-3 text-sm font-bold">
              {RISK_FIGURE_LABEL[f.field]} <span className="font-mono tabular">{usd(f.value)}</span>
            </p>
          ),
        )}
      </section>

      {approval.approvers_required > 1 ? (
        <p className="bg-card px-5 py-3">
          Needs {approval.approvers_required} approvers. Approved so far:{" "}
          {approval.approvals_so_far.length === 0 ? "nobody" : approval.approvals_so_far.map((a) => `${a.user_label} at ${clock(a.at)}`).join(", ")}.
        </p>
      ) : null}

      <Collapsible className="bg-card">
        <CollapsibleTrigger className="group flex w-full items-center justify-between gap-3 px-5 py-4 text-left font-bold">
          View model output
          <ChevronDown className="size-5 transition-transform duration-(--duration-hover) ease-(--d-ease-in-out) group-data-[state=open]:rotate-180" aria-hidden />
        </CollapsibleTrigger>
        <CollapsibleContent className="grid gap-4 px-5 pb-5">
          {approval.evidence.map((e) => (
            <figure key={e.model_id} className="grid gap-1.5">
              <figcaption className="text-sm text-muted-foreground">
                {e.author === "owner_selected" ? "Output of software you selected" : <span className="text-orchid-text">Platform-authored</span>}: <span translate="no">{e.model_id} {e.version}</span>, at{" "}
                {clock(e.produced_at)}
              </figcaption>
              <blockquote className="grid gap-0.5 bg-muted px-3 py-2 font-mono text-sm">
                {e.lines.map((line) => (
                  <p key={line}>{line}</p>
                ))}
              </blockquote>
            </figure>
          ))}
        </CollapsibleContent>
      </Collapsible>

      {open ? (
        <section
          aria-label="Your response"
          className="sticky bottom-[calc(3.5rem+1px+env(safe-area-inset-bottom))] z-10 -mx-4 mt-4 grid gap-3 bg-muted px-4 py-4 sm:static sm:mx-0 sm:px-5"
        >
          <p className="font-display text-2xl leading-tight font-extrabold">If you do nothing, this action is skipped.</p>
          <Deadline deadline={approval.deadline} now={now} />
          {response ? (
            <div role="status" aria-live="polite" className="grid gap-2">
              <ol className="grid grid-cols-3 gap-1.5 text-sm" aria-label="Where your response stands">
                {STAGES.map((s, i) => (
                  <li
                    key={s}
                    aria-current={i === stage ? "step" : undefined}
                    className={cn("px-2 py-1.5 font-bold", i === stage ? "bg-(--lapis) text-(--lapis-foreground)" : i < stage ? "bg-ink text-ink-foreground" : "bg-card text-muted-foreground")}
                  >
                    {s}
                  </li>
                ))}
              </ol>
              <p>
                <ResponseText approval={approval} response={response} />
              </p>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-1.5" data-slot="approval-choices">
              <button type="button" className={CHOICE} onClick={() => respond("approve")}>
                Approve
              </button>
              <button type="button" className={CHOICE} onClick={() => respond("skip")}>
                Skip
              </button>
            </div>
          )}
        </section>
      ) : outcome ? (
        <section aria-label="Outcome" data-status={approval.status} className="mt-4 grid gap-1 bg-muted px-5 py-4">
          <p className="font-display text-2xl font-extrabold uppercase">{outcome.title}</p>
          <p>{outcome.text}</p>
        </section>
      ) : null}
    </article>
  );
}

export function PlacardDashboard() {
  return (
    <WorkspaceGate>
      <Dashboard />
    </WorkspaceGate>
  );
}

export function PlacardApproval({ approvalId }: { approvalId: string }) {
  return (
    <WorkspaceGate>
      <Approval approvalId={approvalId} />
    </WorkspaceGate>
  );
}
