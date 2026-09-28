"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { ArrowLeft, ArrowRight, ChevronDown, CircleAlert } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "@/lib/utils";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Deadline } from "@/components/approvals/deadline";
import { Placeholder } from "@/components/domain/placeholders";
import { AsOf } from "@/components/domain/as-of";
import { WorkspaceGate } from "@/components/screens/common";
import type { Agent, AgentMode } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { type Dec, dec, ratio } from "@/lib/decimal";
import { clock, price, quantity, usd } from "@/lib/format";
import { actionSentence, gateRule, verdictLabel } from "@/lib/gate-reasons";
import { MODE_LABEL, PURPOSE_LABEL, RISK_CAP_LABEL, RISK_FIGURE_LABEL } from "@/lib/labels";
import { agentLimits } from "@/lib/limits";
import { describeRestriction } from "@/lib/restrictions";
import { Figure, ResponseText, STAGES, Signed, outcomeOf, stageOf, useApproval, useDashboard } from "./shared";

const MODE_TONE: Record<AgentMode, string> = {
  normal: "text-muted-foreground",
  exits_only: "text-(--signal-text) font-semibold",
  paused: "text-(--signal-text) font-semibold",
  stopped: "text-foreground font-semibold",
};

const TICKS = [10, 20, 30, 40, 50, 60, 70, 80, 90];

/** A limit read like a scale: usage in ink along ruled ticks, the limit as an orange wall. */
function Rail({ label, used, cap }: { label: string; used: Dec; cap: Dec }) {
  const share = Math.min(ratio(used, cap), 1);
  const over = used > cap;
  return (
    <div className="grid min-w-[8.5rem] gap-1" data-slot="limit-rail">
      <div className="flex items-baseline justify-between gap-2 font-mono text-[0.8125rem] tabular">
        <Figure value={usd(used, 0)} />
        <span className="text-muted-foreground">/ {usd(cap, 0)}</span>
      </div>
      <svg role="img" aria-label={`${label}: ${usd(used)} of a ${usd(cap)} limit`} className="h-2.5 w-full overflow-visible" preserveAspectRatio="none">
        <line x1="0" x2="100%" y1="5" y2="5" className="stroke-border" strokeWidth="1" />
        {TICKS.map((t) => (
          <line key={t} x1={`${t}%`} x2={`${t}%`} y1={t === 50 ? 1 : 3} y2="7" className="stroke-border" strokeWidth="1" />
        ))}
        <rect x="0" y="3.5" height="3" width={`${share * 100}%`} className={over ? "fill-(--signal)" : "fill-foreground"} />
        <rect x="100%" y="0" width="2.5" height="10" transform="translate(-2.5 0)" className="fill-(--signal)" />
      </svg>
    </div>
  );
}

function Mode({ mode }: { mode: AgentMode }) {
  return (
    <span data-mode={mode} className={cn("inline-flex items-center gap-1.5 whitespace-nowrap", MODE_TONE[mode])}>
      <span aria-hidden className={cn("size-1.5 rounded-full", mode === "normal" ? "bg-muted-foreground" : "bg-(--signal)")} />
      {MODE_LABEL[mode]}
    </span>
  );
}

function RestrictionLines({ agent }: { agent: Agent }) {
  return (
    <AnimatePresence initial={false}>
      {agent.restrictions.map((r) => {
        const d = describeRestriction(r);
        return (
          <motion.p
            key={`${r.code}-${r.symbol ?? ""}`}
            layout
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.16, ease: [0.23, 1, 0.32, 1] }}
            className="flex items-start gap-2 text-[0.8125rem]"
          >
            <CircleAlert aria-hidden className="mt-0.5 size-3.5 shrink-0 text-(--signal-text)" />
            <span>
              <span className="font-semibold">{d.title}.</span> Blocks {d.blocks.toLowerCase()}. Ends when {d.endsWhen.charAt(0).toLowerCase() + d.endsWhen.slice(1)}.
            </span>
          </motion.p>
        );
      })}
    </AnimatePresence>
  );
}

const TH = "py-2 pr-4 text-left align-bottom text-[0.75rem] font-medium text-muted-foreground [font-variation-settings:'wdth'_80]";

function AgentsTable() {
  const { ws, now, marketStale } = useDashboard();
  return (
    <>
      <table className="hidden w-full border-collapse md:table">
        <caption className="sr-only">Agents, with their limits in dollars</caption>
        <thead>
          <tr className="border-y border-foreground/70">
            <th scope="col" className={TH}>Agent</th>
            <th scope="col" className={TH}>Mode</th>
            <th scope="col" className={cn(TH, "text-right")}>Equity</th>
            <th scope="col" className={cn(TH, "text-right")}>
              <span className="grid justify-items-end gap-1">
                Paper P&amp;L, simulated
                <Placeholder name="performance" className="rounded-none whitespace-nowrap" />
              </span>
            </th>
            <th scope="col" className={TH}>Gross exposure</th>
            <th scope="col" className={TH}>Daily loss</th>
            <th scope="col" className={cn(TH, "pr-0 text-right")}>Holding</th>
          </tr>
        </thead>
        {ws.agents.map((agent) => {
          const limits = agentLimits(agent);
          const gross = limits.rails.find((r) => r.key === "gross");
          const daily = limits.rails.find((r) => r.key === "daily");
          const markAt = agent.positions[0]?.mark_as_of;
          return (
            <motion.tbody key={agent.agent_id} layout="position" transition={{ duration: 0.18, ease: [0.23, 1, 0.32, 1] }} className="border-b border-border hover:bg-muted/60">
              <tr>
                <th scope="row" className="py-3 pr-4 text-left align-top font-normal">
                  <Link href={`/agents/${agent.agent_id}`} className="font-semibold underline-offset-4 hover:underline">
                    {agent.label}
                  </Link>
                  <span className="block text-[0.8125rem] text-muted-foreground">{agent.mandate.name}</span>
                </th>
                <td className="py-3 pr-4 align-top text-sm">
                  <Mode mode={agent.mode} />
                </td>
                <td className="py-3 pr-4 text-right align-top font-mono text-sm tabular">
                  <Figure value={usd(agent.state.equity)} />
                  <span className="block text-[0.75rem] text-muted-foreground">of {usd(agent.mandate.capital.allocation_usd, 0)}</span>
                </td>
                <td className="py-3 pr-4 text-right align-top text-sm">
                  <Signed value={agent.pnl_total} />
                  <span className="block text-[0.75rem] text-muted-foreground">
                    today <Signed value={agent.pnl_today} word={false} />
                  </span>
                  {marketStale && markAt ? <AsOf at={markAt} now={now} stale className="block text-[0.75rem]" /> : null}
                </td>
                <td className="py-3 pr-4 align-top">{gross ? <Rail label={gross.label} used={gross.used} cap={gross.cap} /> : null}</td>
                <td className="py-3 pr-4 align-top">{daily ? <Rail label={daily.label} used={daily.used} cap={daily.cap} /> : null}</td>
                <td className="py-3 text-right align-top text-sm">
                  {agent.positions.length === 0 ? (
                    <span className="text-muted-foreground">Flat</span>
                  ) : (
                    agent.positions.map((p) => (
                      <span key={p.instrument.asset_id} className="block whitespace-nowrap">
                        <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                      </span>
                    ))
                  )}
                </td>
              </tr>
              {agent.restrictions.length > 0 ? (
                <tr>
                  <td colSpan={7} className="grid gap-1 pb-3">
                    <RestrictionLines agent={agent} />
                  </td>
                </tr>
              ) : null}
            </motion.tbody>
          );
        })}
      </table>

      <ul className="grid border-t border-foreground/70 md:hidden" aria-label="Agents">
        {ws.agents.map((agent) => {
          const limits = agentLimits(agent);
          return (
            <motion.li key={agent.agent_id} layout="position" transition={{ duration: 0.18, ease: [0.23, 1, 0.32, 1] }} className="grid gap-3 border-b border-border py-4">
              <div className="flex items-baseline justify-between gap-3">
                <Link href={`/agents/${agent.agent_id}`} className="font-semibold underline-offset-4 hover:underline">
                  {agent.label}
                </Link>
                <Mode mode={agent.mode} />
              </div>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                <dt className="text-muted-foreground">Equity</dt>
                <dd className="text-right font-mono tabular">
                  <Figure value={usd(agent.state.equity)} />
                </dd>
                <dt className="grid gap-1 text-muted-foreground">
                  Paper P&amp;L, simulated
                  <Placeholder name="performance" className="w-fit rounded-none" />
                </dt>
                <dd className="text-right">
                  <Signed value={agent.pnl_total} />
                </dd>
              </dl>
              <div className="grid grid-cols-2 gap-4">
                {limits.rails
                  .filter((r) => r.key === "gross" || r.key === "daily")
                  .map((r) => (
                    <div key={r.key} className="grid gap-1">
                      <span className="text-[0.75rem] text-muted-foreground">{r.label}</span>
                      <Rail label={r.label} used={r.used} cap={r.cap} />
                    </div>
                  ))}
              </div>
              {agent.restrictions.length > 0 ? (
                <div className="grid gap-1">
                  <RestrictionLines agent={agent} />
                </div>
              ) : null}
            </motion.li>
          );
        })}
      </ul>
    </>
  );
}

function SectionTitle({ id, children, aside }: { id: string; children: string; aside?: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3 pb-2">
      <h2 id={id} className="text-xl font-semibold [font-variation-settings:'wdth'_110]">
        {children}
      </h2>
      {aside}
    </div>
  );
}

function Dashboard() {
  const { ws, now, open, running, positions } = useDashboard();
  if (ws.agents.length === 0) {
    return (
      <section aria-labelledby="v-empty" className="grid max-w-xl gap-4 border-t border-foreground/70 pt-6">
        <h1 id="v-empty" className="text-3xl font-semibold [font-variation-settings:'wdth'_112]">
          No agents yet
        </h1>
        <p className="text-muted-foreground">An agent trades on paper within a mandate you describe and confirm, field by field.</p>
        <Link href="/agents/new" className="d-press inline-flex h-10 w-fit items-center gap-2 rounded-(--radius) bg-foreground px-4 font-semibold text-background hover:bg-foreground/85">
          Describe your first agent <ArrowRight aria-hidden className="size-4" />
        </Link>
      </section>
    );
  }
  return (
    <div className="grid gap-10">
      <header className="grid gap-5">
        <h1 className="text-[2rem] leading-none font-semibold [font-variation-settings:'wdth'_112]">Dashboard</h1>
        <dl className="grid grid-cols-2 border-y border-foreground/70 sm:grid-cols-4">
          {(
            [
              ["Account equity", <Figure key="e" value={usd(ws.connection.account_equity)} />, false],
              [`Agents on ${ws.connection.broker}`, String(ws.agents.length), false],
              ["Running", `${running} of ${ws.agents.length}`, false],
              ["Waiting for you", open.length === 0 ? "Nothing" : `${open.length} request${open.length === 1 ? "" : "s"}`, open.length > 0],
            ] as Array<[string, ReactNode, boolean]>
          ).map(([label, value, signal], i) => (
            <div key={label} className={cn("grid gap-1 py-3 pr-4", i > 0 && "sm:border-l sm:border-border sm:pl-4", i % 2 === 1 && "border-l border-border pl-4", i > 1 && "border-t border-border sm:border-t-0")}>
              <dt className="text-[0.75rem] text-muted-foreground">{label}</dt>
              <dd className={cn("font-mono text-lg tabular", signal && "font-semibold text-(--signal-text)")}>{value}</dd>
            </div>
          ))}
        </dl>
      </header>

      {open.length > 0 ? (
        <section aria-labelledby="v-waiting">
          <SectionTitle id="v-waiting">Waiting for you</SectionTitle>
          <ul className="border-t border-foreground/70">
            {open.map((a) => {
              const agent = findAgent(ws, a.agent_id);
              return (
                <li key={a.approval_id} className="border-b border-border">
                  <Link
                    href={`/approvals/${a.approval_id}`}
                    className="group grid gap-1 py-3 hover:bg-muted/60 sm:grid-cols-[1fr_auto_auto] sm:items-center sm:gap-6"
                  >
                    <span className="flex items-baseline gap-2.5 font-medium">
                      <span aria-hidden className="size-2 shrink-0 translate-y-[-1px] bg-(--signal)" />
                      <span>
                      {agent?.label ?? "An agent"} asks to buy <span className="font-mono tabular">{quantity(a.bound.qty)}</span> {a.bound.symbol} at{" "}
                      <span className="font-mono tabular">{price(a.bound.limit)}</span>
                      </span>
                    </span>
                    <Deadline deadline={a.deadline} now={now} className="text-muted-foreground" />
                    <ArrowRight aria-hidden className="hidden size-4 text-muted-foreground transition-transform duration-(--duration-hover) [@media(hover:hover)]:group-hover:translate-x-0.5 sm:block" />
                  </Link>
                </li>
              );
            })}
          </ul>
        </section>
      ) : null}

      <section aria-labelledby="v-agents">
        <SectionTitle id="v-agents" aside={<span className="text-[0.75rem] text-muted-foreground">Limits in dollars; orange marks where each limit stops the agent</span>}>
          Agents
        </SectionTitle>
        <AgentsTable />
      </section>

      <div className="grid gap-10 lg:grid-cols-[3fr_2fr]">
        <section aria-labelledby="v-decisions">
          <SectionTitle id="v-decisions">Recent gate decisions</SectionTitle>
          {ws.decisions.length === 0 ? (
            <p className="border-t border-foreground/70 py-3 text-sm text-muted-foreground">No decisions yet.</p>
          ) : (
            <ol className="border-t border-foreground/70">
              {ws.decisions.slice(0, 6).map((d) => {
                const agent = findAgent(ws, d.agent_id);
                const rule = d.reason_code && agent ? gateRule(d.reason_code, agent.mandate) : null;
                return (
                  <li key={d.event_id} data-verdict={d.verdict} className="grid grid-cols-[3rem_5.5rem_1fr] gap-3 border-b border-border py-2.5 text-sm">
                    <time dateTime={d.at} className="font-mono text-[0.8125rem] text-muted-foreground tabular">
                      {clock(d.at).slice(0, 5)}
                    </time>
                    <span className={cn("text-[0.8125rem]", d.verdict === "allow" ? "text-muted-foreground" : "font-semibold text-(--signal-text)")}>{verdictLabel(d)}</span>
                    <span className="grid gap-0.5">
                      <span>
                        {actionSentence(d.action)} <span className="text-muted-foreground">· {agent?.label}</span>
                      </span>
                      {rule ? <span className="text-[0.8125rem] text-muted-foreground">{rule}</span> : null}
                      {d.then ? <span className="text-[0.8125rem] text-muted-foreground">{d.then}</span> : null}
                    </span>
                  </li>
                );
              })}
            </ol>
          )}
        </section>

        <section aria-labelledby="v-positions">
          <SectionTitle id="v-positions">Positions</SectionTitle>
          {positions.length === 0 ? (
            <p className="border-t border-foreground/70 py-3 text-sm text-muted-foreground">No agent holds a position.</p>
          ) : (
            <table className="w-full border-collapse text-sm">
              <caption className="sr-only">Positions across agents</caption>
              <thead>
                <tr className="border-y border-foreground/70">
                  <th scope="col" className={TH}>Holding</th>
                  <th scope="col" className={cn(TH, "text-right")}>Value</th>
                  <th scope="col" className={cn(TH, "pr-0 text-right")}>Unrealized</th>
                </tr>
              </thead>
              <tbody>
                {positions.map(({ agent, p }) => (
                  <tr key={`${agent.agent_id}-${p.instrument.asset_id}`} className="border-b border-border">
                    <th scope="row" className="py-2.5 pr-4 text-left font-normal">
                      <span className="font-mono tabular">{quantity(p.qty)}</span> {p.instrument.symbol}
                      <span className="block text-[0.75rem] text-muted-foreground">{agent.label}</span>
                    </th>
                    <td className="py-2.5 pr-4 text-right align-top font-mono tabular">
                      <Figure value={usd(p.market_value)} />
                    </td>
                    <td className="py-2.5 text-right align-top">
                      <Signed value={p.unrealized_pnl} word={false} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {ws.external_positions.length > 0 ? (
            <p className="pt-3 text-[0.8125rem] text-muted-foreground">
              Also on the account, not managed by any agent: {ws.external_positions.map((e) => `${quantity(e.qty)} ${e.instrument.symbol}`).join(", ")}.
            </p>
          ) : null}
        </section>
      </div>
    </div>
  );
}

const CHOICE =
  "d-press h-12 w-full rounded-(--radius) border border-foreground bg-background text-base font-semibold text-foreground hover:bg-muted focus-visible:outline-2 focus-visible:outline-offset-2";

function Row({ label, children, className }: { label: ReactNode; children: ReactNode; className?: string }) {
  return (
    <div className={cn("grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-4 border-b border-border py-2.5", className)}>
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd className="text-right">{children}</dd>
    </div>
  );
}

function Approval({ approvalId }: { approvalId: string }) {
  const view = useApproval(approvalId);
  if (!view) return <p className="text-muted-foreground">No approval request with this ID.</p>;
  const { approval, agent, response, respond, orderValue, open, now } = view;
  const b = approval.bound;
  const stage = stageOf(approval, response);
  const outcome = outcomeOf(approval);
  return (
    <article className="mx-auto grid w-full max-w-xl gap-6" aria-labelledby="v-request">
      <Link href="/approvals" className="inline-flex w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
        <ArrowLeft className="size-4" aria-hidden /> Approvals
      </Link>
      <header className="grid gap-1 border-t border-foreground/70 pt-3">
        <h1 id="v-request" className="text-[2rem] leading-tight font-semibold [font-variation-settings:'wdth'_110]">
          Buy <span className="font-mono tabular">{quantity(b.qty)}</span> {b.symbol}
        </h1>
        <p className="text-lg">
          at a limit of <span className="font-mono font-semibold tabular">{price(b.limit)}</span>
        </p>
        <p className="text-sm text-muted-foreground">
          Approval request from {agent?.label ?? "an agent"} ({agent?.mandate.name ?? "unknown mandate"})
        </p>
      </header>

      <dl className="border-t border-foreground/70">
        <Row label="Order value">
          <span className="font-mono tabular">{orderValue}</span>
        </Row>
        <Row label="Purpose">{PURPOSE_LABEL[b.purpose]}</Row>
        <Row label="Mandate version">
          <span translate="no" className="font-mono text-sm">{b.mandate_version.slice(7, 19)}</span>
        </Row>
        <div className="grid gap-1 border-b border-border py-2.5">
          <dt className="text-sm text-muted-foreground">Why you are asked</dt>
          <dd>{approval.trigger}</dd>
        </div>
        <Row label="Combined model score, not a probability of profit">
          <span className="font-mono tabular">{b.combined_score}</span>
        </Row>
      </dl>

      <section aria-labelledby="v-risk" className="grid gap-2">
        <h2 id="v-risk" className="text-xl font-semibold [font-variation-settings:'wdth'_110]">
          Risk impact in dollars
        </h2>
        <dl className="border-t border-foreground/70">
          {approval.risk_impact.map((f) => (
            <div key={f.field} className="grid gap-2 border-b border-border py-2.5">
              <div className="flex items-baseline justify-between gap-4">
                <dt className="text-sm">{RISK_FIGURE_LABEL[f.field]}</dt>
                <dd className="text-right font-mono text-sm tabular">
                  {usd(f.value)}
                  {f.cap ? <span className="block font-sans text-[0.75rem] text-muted-foreground">of the {usd(f.cap)} {RISK_CAP_LABEL[f.field]}</span> : null}
                </dd>
              </div>
              {f.cap ? <Rail label={RISK_FIGURE_LABEL[f.field]} used={dec(f.value)} cap={dec(f.cap)} /> : null}
            </div>
          ))}
        </dl>
      </section>

      {approval.approvers_required > 1 ? (
        <p className="text-sm">
          Needs {approval.approvers_required} approvers. Approved so far:{" "}
          {approval.approvals_so_far.length === 0 ? "nobody" : approval.approvals_so_far.map((a) => `${a.user_label} at ${clock(a.at)}`).join(", ")}.
        </p>
      ) : null}

      <Collapsible className="border-y border-border">
        <CollapsibleTrigger className="group flex w-full items-center justify-between gap-3 py-3 text-left font-medium">
          View model output
          <ChevronDown className="size-4 transition-transform duration-(--duration-hover) ease-(--d-ease-in-out) group-data-[state=open]:rotate-180" aria-hidden />
        </CollapsibleTrigger>
        <CollapsibleContent className="grid gap-4 pb-4">
          {approval.evidence.map((e) => (
            <figure key={e.model_id} className="grid gap-1.5">
              <figcaption className="text-[0.8125rem] text-muted-foreground">
                {e.author === "owner_selected" ? "Output of software you selected" : <span className="text-orchid-text">Platform-authored</span>}: <span translate="no">{e.model_id} {e.version}</span>, at{" "}
                {clock(e.produced_at)}
              </figcaption>
              <blockquote className="grid gap-0.5 border-l border-foreground/40 pl-3 font-mono text-[0.8125rem]">
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
          className="sticky bottom-[calc(3.5rem+1px+env(safe-area-inset-bottom))] z-10 -mx-4 grid gap-3 border-t border-foreground/70 bg-background px-4 py-4 sm:static sm:mx-0 sm:px-0"
        >
          <p className="font-semibold">If you do nothing, this action is skipped.</p>
          <Deadline deadline={approval.deadline} now={now} />
          {response ? (
            <div role="status" aria-live="polite" className="grid gap-2 text-sm">
              <ol className="flex gap-4 text-[0.75rem]" aria-label="Where your response stands">
                {STAGES.map((s, i) => (
                  <li key={s} aria-current={i === stage ? "step" : undefined} className={cn("flex items-center gap-1.5", i <= stage ? "text-foreground" : "text-muted-foreground")}>
                    <span aria-hidden className={cn("h-2 w-2", i === stage ? "bg-(--signal)" : i < stage ? "bg-foreground" : "border border-border")} />
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
        <section aria-label="Outcome" data-status={approval.status} className="grid gap-1 border-t border-foreground/70 pt-3">
          <p className="font-semibold">{outcome.title}</p>
          <p className="text-sm">{outcome.text}</p>
        </section>
      ) : null}
    </article>
  );
}

export function VernierDashboard() {
  return (
    <WorkspaceGate>
      <Dashboard />
    </WorkspaceGate>
  );
}

export function VernierApproval({ approvalId }: { approvalId: string }) {
  return (
    <WorkspaceGate>
      <Approval approvalId={approvalId} />
    </WorkspaceGate>
  );
}
