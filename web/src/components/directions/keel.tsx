"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { ArrowLeft, ArrowRight, ChevronDown, CircleAlert } from "lucide-react";
import { cn } from "@/lib/utils";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Deadline } from "@/components/approvals/deadline";
import { Placeholder } from "@/components/domain/placeholders";
import { WorkspaceGate } from "@/components/screens/common";
import type { Agent, AgentMode } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { type Dec, add, dec, ratio } from "@/lib/decimal";
import { clock, price, quantity, usd } from "@/lib/format";
import { actionSentence, gateRule, verdictLabel } from "@/lib/gate-reasons";
import { MODE_LABEL, PURPOSE_LABEL, RISK_CAP_LABEL, RISK_FIGURE_LABEL } from "@/lib/labels";
import { agentLimits } from "@/lib/limits";
import { describeRestriction } from "@/lib/restrictions";
import { Figure, ResponseText, STAGES, Signed, outcomeOf, stageOf, stagger, useApproval, useDashboard } from "./shared";

/** One elevation, used for every panel; nothing sits in a box inside a panel. */
const PANEL = "rounded-(--radius) bg-card shadow-[0_1px_0_oklch(0.25_0.02_55/0.06),0_8px_24px_-12px_oklch(0.25_0.02_55/0.18)]";

const MODE_PILL: Record<AgentMode, string> = {
  normal: "bg-(--petrol-soft) text-foreground",
  exits_only: "bg-(--apricot-soft) text-foreground",
  paused: "bg-(--apricot-soft) text-foreground",
  stopped: "bg-ink text-ink-foreground",
};

function ModePill({ mode }: { mode: AgentMode }) {
  return (
    <span data-mode={mode} className={cn("inline-flex h-7 items-center gap-1.5 rounded-full px-3 text-sm font-medium whitespace-nowrap", MODE_PILL[mode])}>
      <span aria-hidden className={cn("size-2 rounded-full", mode === "normal" ? "bg-(--petrol)" : mode === "stopped" ? "bg-ink-foreground" : "bg-(--apricot)")} />
      {MODE_LABEL[mode]}
    </span>
  );
}

/** A slider you set: the petrol track and knob are the usage, the apricot notch is where it stops the agent. */
function Meter({ label, used, cap }: { label: string; used: Dec; cap: Dec }) {
  const share = Math.min(ratio(used, cap), 1);
  return (
    <div className="grid gap-2" data-slot="limit-rail">
      <div className="flex items-baseline justify-between gap-3 text-sm">
        <span className="text-muted-foreground">{label}</span>
        <span className="font-mono tabular">
          <Figure value={usd(used, 0)} /> <span className="text-muted-foreground">of {usd(cap, 0)}</span>
        </span>
      </div>
      <div role="img" aria-label={`${label}: ${usd(used)} of a ${usd(cap)} limit`} className="relative mx-1.5 h-1.5 rounded-full bg-muted">
        <div className="absolute inset-y-0 left-0 rounded-full bg-(--petrol)" style={{ width: `${share * 100}%` }} />
        <div className="absolute -inset-y-1.5 right-0 w-1 translate-x-1/2 rounded-full bg-(--apricot)" />
        <div
          className="absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-(--petrol) bg-card shadow-[0_1px_2px_oklch(0.25_0.02_55/0.3)]"
          style={{ left: `${share * 100}%` }}
        />
      </div>
    </div>
  );
}

function PanelTitle({ id, children, aside }: { id: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <h2 id={id} className="text-xl font-semibold tracking-tight">
        {children}
      </h2>
      {aside}
    </div>
  );
}

function AgentModule({ agent, index }: { agent: Agent; index: number }) {
  const { now, marketStale } = useDashboard();
  const limits = agentLimits(agent);
  const rails = limits.rails.filter((r) => r.key === "gross" || r.key === "daily");
  const markAt = agent.positions[0]?.mark_as_of;
  return (
    <li className="d-reveal grid content-start gap-5 border-t border-border p-5 first:border-t-0 md:border-t-0 md:border-l md:first:border-l-0" style={stagger(index)}>
      <div className="flex items-start justify-between gap-3">
        <div className="grid gap-0.5">
          <Link href={`/agents/${agent.agent_id}`} className="text-lg font-semibold underline-offset-4 hover:underline">
            {agent.label}
          </Link>
          <span className="text-sm text-muted-foreground">{agent.mandate.name}</span>
        </div>
        <ModePill mode={agent.mode} />
      </div>
      <div className="grid gap-1">
        <p className="font-display text-[2.25rem] leading-none font-semibold tracking-tight">
          <Figure value={usd(agent.state.equity)} />
        </p>
        <p className="text-sm text-muted-foreground">equity, of {usd(agent.mandate.capital.allocation_usd, 0)} allocated</p>
      </div>
      <div className="grid gap-1.5 text-sm">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <span>Paper P&amp;L, simulated</span>
          <Signed value={agent.pnl_total} className="font-semibold" />
        </div>
        <div className="flex flex-wrap items-baseline justify-between gap-2 text-muted-foreground">
          <Placeholder name="performance" className="rounded-full" />
          <span>
            today <Signed value={agent.pnl_today} word={false} />
          </span>
        </div>
        {marketStale && markAt ? (
          <p className="text-muted-foreground">
            Marks as of {clock(markAt)}; market data is behind ({clock(now)} now).
          </p>
        ) : null}
      </div>
      <div className="grid gap-4">
        {rails.map((r) => (
          <Meter key={r.key} label={r.label} used={r.used} cap={r.cap} />
        ))}
      </div>
      <div className="grid gap-2 text-sm">
        {agent.positions.length === 0 ? (
          <p className="text-muted-foreground">Flat, no positions.</p>
        ) : (
          <p>Holds {agent.positions.map((p) => `${quantity(p.qty)} ${p.instrument.symbol}`).join(", ")}</p>
        )}
        {agent.restrictions.map((r) => {
          const d = describeRestriction(r);
          return (
            <p key={`${r.code}-${r.symbol ?? ""}`} className="flex items-start gap-2">
              <CircleAlert aria-hidden className="mt-0.5 size-4 shrink-0 text-(--persimmon-text)" />
              <span>
                <span className="font-semibold">{d.title}.</span> Blocks {d.blocks.toLowerCase()}. Ends when{" "}
                {d.endsWhen.charAt(0).toLowerCase() + d.endsWhen.slice(1)}.
              </span>
            </p>
          );
        })}
      </div>
    </li>
  );
}

const LIGHT: Record<AgentMode, string> = {
  normal: "bg-(--petrol-foreground)",
  exits_only: "bg-(--apricot)",
  paused: "bg-(--apricot)",
  stopped: "border border-(--petrol-foreground)",
};

function Dashboard() {
  const { ws, now, open, running, positions } = useDashboard();
  if (ws.agents.length === 0) {
    return (
      <section aria-labelledby="k-empty" className={cn(PANEL, "grid max-w-xl gap-4 p-8")}>
        <h1 id="k-empty" className="text-3xl font-semibold tracking-tight">
          No agents yet
        </h1>
        <p className="text-muted-foreground">An agent trades on paper within a mandate you describe and confirm, field by field.</p>
        <Link
          href="/agents/new"
          className="d-press inline-flex h-11 w-fit items-center gap-2 rounded-full bg-(--petrol) px-5 font-semibold text-(--petrol-foreground) hover:bg-(--petrol)/90"
        >
          Describe your first agent <ArrowRight aria-hidden className="size-4" />
        </Link>
      </section>
    );
  }
  return (
    <div className="grid gap-6">
      <h1 className="text-[2.5rem] leading-tight font-semibold tracking-tight">Dashboard</h1>

      <section aria-label="The account and what is waiting" className={cn(PANEL, "d-reveal grid overflow-hidden lg:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)]")} style={stagger(0)}>
        <div className="grid content-start gap-4 bg-(--petrol) p-6 text-(--petrol-foreground)">
          <div className="grid gap-1">
            <h2 className="text-base font-medium">Account equity, {ws.connection.broker}</h2>
            <p className="font-display text-[2.75rem] leading-none font-semibold tracking-tight">
              <Figure value={usd(ws.connection.account_equity)} />
            </p>
            <p className="text-sm">
              {usd(add(...ws.agents.map((a) => dec(a.mandate.capital.allocation_usd))), 0)} allocated to {ws.agents.length} agents; {running} running.
            </p>
          </div>
          <ul className="flex flex-wrap gap-x-5 gap-y-2 text-sm" aria-label="Agent modes">
            {ws.agents.map((a) => (
              <li key={a.agent_id} className="inline-flex items-center gap-2">
                <span aria-hidden className={cn("size-2.5 rounded-full", LIGHT[a.mode])} />
                {a.label}, {MODE_LABEL[a.mode].toLowerCase()}
              </li>
            ))}
          </ul>
        </div>
        <div className="grid content-start gap-2 p-6">
          <PanelTitle
            id="k-waiting"
            aside={open.length > 0 ? <span className="rounded-full bg-(--apricot-soft) px-2.5 py-0.5 text-sm font-medium">{open.length} open</span> : null}
          >
            Waiting for you
          </PanelTitle>
          {open.length === 0 ? (
            <p className="text-muted-foreground">Nothing needs your approval.</p>
          ) : (
            <ul className="grid">
              {open.map((a) => {
                const agent = findAgent(ws, a.agent_id);
                return (
                  <li key={a.approval_id} className="border-t border-border first:border-t-0">
                    <Link
                      href={`/approvals/${a.approval_id}`}
                      className="group -mx-3 grid gap-1 rounded-[calc(var(--radius)-0.25rem)] px-3 py-3 transition-colors duration-(--duration-hover) hover:bg-muted sm:grid-cols-[1fr_auto] sm:items-center sm:gap-4"
                    >
                      <span className="grid gap-0.5">
                        <span className="font-medium">
                          {agent?.label ?? "An agent"} asks to buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at{" "}
                          <span className="font-mono tabular">{price(a.bound.limit)}</span>
                        </span>
                        <Deadline deadline={a.deadline} now={now} className="text-sm text-muted-foreground" />
                      </span>
                      <span className="inline-flex h-9 items-center gap-1 rounded-full bg-(--apricot-soft) px-3.5 text-sm font-semibold">
                        Review
                        <ArrowRight aria-hidden className="size-4 transition-transform duration-(--duration-hover) [@media(hover:hover)]:group-hover:translate-x-0.5" />
                      </span>
                    </Link>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </section>

      <section aria-labelledby="k-agents" className={cn(PANEL, "grid")}>
        <div className="border-b border-border px-5 py-4">
          <PanelTitle id="k-agents">Agents</PanelTitle>
        </div>
        <ul className="grid md:grid-cols-3">
          {ws.agents.map((agent, i) => (
            <AgentModule key={agent.agent_id} agent={agent} index={i + 1} />
          ))}
        </ul>
      </section>

      <div className={cn(PANEL, "grid lg:grid-cols-[3fr_2fr]")}>
        <section aria-labelledby="k-decisions" className="grid content-start gap-2 p-5">
          <PanelTitle id="k-decisions">Recent gate decisions</PanelTitle>
          {ws.decisions.length === 0 ? (
            <p className="text-muted-foreground">No decisions yet.</p>
          ) : (
            <ol className="grid">
              {ws.decisions.slice(0, 6).map((d) => {
                const agent = findAgent(ws, d.agent_id);
                const rule = d.reason_code && agent ? gateRule(d.reason_code, agent.mandate) : null;
                const blocked = d.verdict !== "allow";
                return (
                  <li key={d.event_id} data-verdict={d.verdict} className="grid grid-cols-[3rem_minmax(0,1fr)] gap-3 border-t border-border py-3 text-sm first:border-t-0">
                    <time dateTime={d.at} className="font-mono text-muted-foreground tabular">
                      {clock(d.at).slice(0, 5)}
                    </time>
                    <div className="grid gap-0.5">
                      <p>
                        <span className={cn("font-semibold", blocked && "text-(--persimmon-text)")}>{verdictLabel(d)}</span> · {actionSentence(d.action)}{" "}
                        <span className="text-muted-foreground">· {agent?.label}</span>
                      </p>
                      {rule ? <p className="text-muted-foreground">{rule}</p> : null}
                      {d.then ? <p className="text-muted-foreground">{d.then}</p> : null}
                    </div>
                  </li>
                );
              })}
            </ol>
          )}
        </section>

        <section aria-labelledby="k-positions" className="grid content-start gap-2 border-t border-border p-5 lg:border-t-0 lg:border-l">
          <PanelTitle id="k-positions">Positions</PanelTitle>
          {positions.length === 0 ? (
            <p className="text-muted-foreground">No agent holds a position.</p>
          ) : (
            <ul className="grid">
              {positions.map(({ agent, p }) => (
                <li key={`${agent.agent_id}-${p.instrument.asset_id}`} className="grid grid-cols-[1fr_auto] gap-x-4 border-t border-border py-3 text-sm first:border-t-0">
                  <span className="font-medium">
                    <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                  </span>
                  <span className="text-right font-mono tabular">
                    <Figure value={usd(p.market_value)} />
                  </span>
                  <span className="text-muted-foreground">{agent.label}</span>
                  <span className="text-right">
                    <Signed value={p.unrealized_pnl} word={false} />
                  </span>
                </li>
              ))}
            </ul>
          )}
          {ws.external_positions.length > 0 ? (
            <p className="border-t border-border pt-3 text-sm text-muted-foreground">
              Also on the account, not managed by any agent: {ws.external_positions.map((e) => `${quantity(e.qty)} ${e.instrument.symbol}`).join(", ")}.
            </p>
          ) : null}
        </section>
      </div>
    </div>
  );
}

const CHOICE =
  "d-press h-13 w-full rounded-full border border-foreground/25 bg-card text-base font-semibold text-foreground shadow-[0_1px_0_oklch(0.25_0.02_55/0.08)] hover:bg-muted focus-visible:outline-2 focus-visible:outline-offset-2";

function Approval({ approvalId }: { approvalId: string }) {
  const view = useApproval(approvalId);
  if (!view) return <p className="text-muted-foreground">No approval request with this ID.</p>;
  const { approval, agent, response, respond, orderValue, open, now } = view;
  const b = approval.bound;
  const stage = stageOf(approval, response);
  const outcome = outcomeOf(approval);
  return (
    <article className="mx-auto grid w-full max-w-xl gap-4" aria-labelledby="k-request">
      <Link href="/approvals" className="inline-flex w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
        <ArrowLeft className="size-4" aria-hidden /> Approvals
      </Link>
      <header className="d-reveal grid gap-1.5 px-1" style={stagger(0)}>
        <h1 id="k-request" className="font-sans text-base font-normal text-muted-foreground">
          {agent?.label ?? "An agent"} ({agent?.mandate.name ?? "unknown mandate"}) asks for your approval
        </h1>
        <p className="font-display text-[2.5rem] leading-[1.05] font-semibold tracking-tight">
          Buy {quantity(b.qty)} {b.symbol}
          <span className="block text-muted-foreground">at {price(b.limit)}</span>
        </p>
      </header>

      <dl className={cn(PANEL, "d-reveal grid gap-0 px-5 py-2")} style={stagger(1)}>
        {(
          [
            ["Order value", <span key="v" className="font-mono tabular">{orderValue}</span>],
            ["Purpose", PURPOSE_LABEL[b.purpose]],
            ["Mandate version", <span key="m" className="font-mono text-sm">{b.mandate_version.slice(7, 19)}</span>],
            ["Combined model score, not a probability of profit", <span key="s" className="font-mono tabular">{b.combined_score}</span>],
          ] as Array<[string, ReactNode]>
        ).map(([label, value]) => (
          <div key={label} className="grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-4 border-t border-border py-3 first:border-t-0">
            <dt className="text-sm text-muted-foreground">{label}</dt>
            <dd className="text-right">{value}</dd>
          </div>
        ))}
        <div className="grid gap-1 border-t border-border py-3">
          <dt className="text-sm text-muted-foreground">Why you are asked</dt>
          <dd className="font-medium">{approval.trigger}</dd>
        </div>
      </dl>

      <section aria-labelledby="k-risk" className={cn(PANEL, "d-reveal grid gap-4 p-5")} style={stagger(2)}>
        <PanelTitle id="k-risk">Against your limits</PanelTitle>
        {approval.risk_impact.map((f) =>
          f.cap ? (
            <div key={f.field} className="grid gap-1">
              <Meter label={RISK_FIGURE_LABEL[f.field]} used={dec(f.value)} cap={dec(f.cap)} />
              <span className="text-[0.8125rem] text-muted-foreground">The {RISK_CAP_LABEL[f.field]}</span>
            </div>
          ) : (
            <p key={f.field} className="flex items-baseline justify-between gap-3 text-sm">
              <span className="text-muted-foreground">{RISK_FIGURE_LABEL[f.field]}</span> <span className="font-mono tabular">{usd(f.value)}</span>
            </p>
          ),
        )}
      </section>

      {approval.approvers_required > 1 ? (
        <p className={cn(PANEL, "px-5 py-3 text-sm")}>
          Needs {approval.approvers_required} approvers. Approved so far:{" "}
          {approval.approvals_so_far.length === 0 ? "nobody" : approval.approvals_so_far.map((a) => `${a.user_label} at ${clock(a.at)}`).join(", ")}.
        </p>
      ) : null}

      <Collapsible className={PANEL}>
        <CollapsibleTrigger className="group flex w-full items-center justify-between gap-3 px-5 py-4 text-left font-semibold">
          View model output
          <ChevronDown className="size-4 transition-transform duration-(--duration-hover) group-data-[state=open]:rotate-180" aria-hidden />
        </CollapsibleTrigger>
        <CollapsibleContent className="grid gap-4 border-t border-border px-5 py-4">
          {approval.evidence.map((e) => (
            <figure key={e.model_id} className="grid gap-1.5">
              <figcaption className="text-sm text-muted-foreground">
                {e.author === "owner_selected" ? "Output of software you selected" : <span className="text-orchid-text">Platform-authored</span>}: {e.model_id} {e.version}, at{" "}
                {clock(e.produced_at)}
              </figcaption>
              <blockquote className="grid gap-0.5 font-mono text-sm text-muted-foreground">
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
          className="sticky bottom-[calc(3.5rem+1px+env(safe-area-inset-bottom))] z-10 -mx-4 grid gap-3 rounded-t-(--radius) bg-card px-5 pt-5 pb-4 shadow-[0_-10px_30px_-18px_oklch(0.25_0.02_55/0.35)] sm:static sm:mx-0 sm:rounded-(--radius)"
        >
          <p className="text-lg font-semibold">If you do nothing, this action is skipped.</p>
          <Deadline deadline={approval.deadline} now={now} className="text-muted-foreground" />
          {response ? (
            <div role="status" aria-live="polite" className="grid gap-2 text-sm">
              <ol className="flex flex-wrap gap-2" aria-label="Where your response stands">
                {STAGES.map((s, i) => (
                  <li
                    key={s}
                    aria-current={i === stage ? "step" : undefined}
                    className={cn(
                      "rounded-full px-3 py-1 font-medium",
                      i === stage ? "bg-(--petrol) text-(--petrol-foreground)" : i < stage ? "bg-(--petrol-soft) text-foreground" : "bg-muted text-muted-foreground",
                    )}
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
            <div className="grid grid-cols-2 gap-3" data-slot="approval-choices">
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
        <section aria-label="Outcome" data-status={approval.status} className={cn(PANEL, "grid gap-1 p-5")}>
          <p className="font-semibold">{outcome.title}</p>
          <p className="text-sm">{outcome.text}</p>
        </section>
      ) : null}
    </article>
  );
}

export function KeelDashboard() {
  return (
    <WorkspaceGate>
      <Dashboard />
    </WorkspaceGate>
  );
}

export function KeelApproval({ approvalId }: { approvalId: string }) {
  return (
    <WorkspaceGate>
      <Approval approvalId={approvalId} />
    </WorkspaceGate>
  );
}
