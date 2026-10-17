"use client";

import { type ReactNode, useEffect, useState } from "react";
import Link from "next/link";
import { type Backtest, type BacktestRead, type BacktestReport as Report, type Disclosure, type Metrics, OUTCOMES, servedBacktest } from "@/api/backtest";
import type { ApiError } from "@/api/types";
import { FixtureTag, Placeholder, type PlaceholderName } from "@/components/domain/placeholders";
import { ScreenSkeleton, Section, WorkspaceGate } from "@/components/screens/common";
import { BACKTESTS_BY_AGENT, servedBacktestFixture } from "@/fixtures/backtests";
import { MINUS } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";

/**
 * One backtest's report (B2; DEC-1001, DEC-1003, DEC-2400), the same screen over the fixtures and over
 * the workspace API: it reads the backtest through the runtime and shows only what was served. Every
 * figure is the served decimal, written out exactly with no rounding and no arithmetic; a statistic the
 * run did not compute says so with its reason code. Codes stay codes and disclosures stay named
 * placeholders: nothing here is drafted legal text, an adjective, a projection, or a thesis-quality
 * figure (DEC-1001 item 3, DEC-1003 item 2). The equity per period is not drawn yet.
 */

/** Each disclosure name the report may carry, as the placeholder that shows it (DEC-1003 item 1). */
const PLACEHOLDER: Record<Disclosure, PlaceholderName> = {
  "LEGEND-HYPOTHETICAL": "hypothetical",
  "NOTE-BACKTEST-MECHANICS": "backtestMechanics",
  "NOTE-BACKTEST-LIMITS": "backtestLimits",
};

const STATE: Record<Backtest["state"], string> = { running: "Running", completed: "Completed", refused: "Refused" };

const CODE = "font-mono text-sm";
const MUTED = "text-muted-foreground";
const ROW_LINK = "press group -mx-3 grid gap-1 rounded-xl px-3 py-3.5 outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring";
const TH = "py-2 pr-4 text-left font-normal text-muted-foreground";
const TD = "py-2.5 pr-4 align-baseline";

/** A served decimal's sign, whole part and fraction, as written: nothing is parsed into a number. */
function parts(value: string): { negative: boolean; whole: string; fraction: string } {
  const negative = value.startsWith("-");
  const [whole, fraction = ""] = (negative ? value.slice(1) : value).split(".");
  return { negative, whole, fraction };
}

const grouped = (whole: string) => whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
const signOf = (negative: boolean) => (negative ? MINUS : "");

/** "10120.5" → "$10,120.50": every served place kept, at least two, the true minus sign. */
export function exactUsd(value: string): string {
  const { negative, whole, fraction } = parts(value);
  return `${signOf(negative)}$${grouped(whole)}.${fraction.padEnd(2, "0")}`;
}

/** A fraction of one as a percentage by moving the point two places: "0.01205" → "1.205%". */
export function exactPercent(value: string): string {
  const { negative, whole, fraction } = parts(value);
  const shifted = `${whole}${fraction.slice(0, 2).padEnd(2, "0")}`.replace(/^0+(?=\d)/, "");
  const rest = fraction.slice(2).replace(/0+$/, "");
  return `${signOf(negative && `${shifted}${rest}` !== "0")}${shifted}${rest ? `.${rest}` : ""}%`;
}

/** A served decimal as written, grouped, with the true minus sign. */
export function exactNumber(value: string): string {
  const { negative, whole, fraction } = parts(value);
  return `${signOf(negative)}${grouped(whole)}${fraction ? `.${fraction}` : ""}`;
}

function Code({ children }: { children: ReactNode }) {
  return <code className={CODE}>{children}</code>;
}

function None() {
  return <span className={MUTED}>None</span>;
}

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="grid min-w-0 gap-1">
      <dt className="text-sm text-muted-foreground">{term}</dt>
      <dd className="break-all">{children}</dd>
    </div>
  );
}

/** A gain or a loss in words beside its sign, so colour never carries it (DESIGN.md); neutral ink, since nothing here steers. */
function NetPnl({ value }: { value: string }) {
  const { negative, whole, fraction } = parts(value);
  const zero = /^0*$/.test(`${whole}${fraction}`);
  const direction = zero ? "flat" : negative ? "loss" : "gain";
  const shown = zero ? exactUsd(value) : `${negative ? MINUS : "+"}${exactUsd(negative ? value.slice(1) : value)}`;
  return (
    <span data-direction={direction} className="font-mono tabular">
      {shown} <span className="font-sans">{zero ? "no change" : direction}</span>
    </span>
  );
}

/** A statistic the run may not compute: `null` is "not computed", with the block's reason code, never a number. */
function Statistic({ value, absent, format }: { value: string | null; absent: Metrics["absent"]; format: (v: string) => string }) {
  if (value !== null) return <span className="font-mono tabular">{format(value)}</span>;
  return (
    <span className={MUTED}>
      Not computed{absent ? ": " : ""}
      {absent ? <Code>{absent}</Code> : null}
    </span>
  );
}

interface MetricRow {
  label: ReactNode;
  key: string;
  cell: (m: Metrics) => ReactNode;
}

const figure = (text: string) => <span className="font-mono tabular">{text}</span>;

/** FR-4.2's figures for the version and for buy-and-hold, with the dates and counts they cover. */
const METRIC_ROWS: MetricRow[] = [
  { key: "first", label: "First date", cell: (m) => figure(m.first_date) },
  { key: "last", label: "Last date", cell: (m) => figure(m.last_date) },
  { key: "periods", label: "Periods", cell: (m) => figure(String(m.period_count)) },
  { key: "per-year", label: "Periods a year", cell: (m) => figure(String(m.periods_per_year)) },
  { key: "start", label: "Starting equity", cell: (m) => figure(exactUsd(m.starting_equity)) },
  { key: "end", label: "Ending equity", cell: (m) => figure(exactUsd(m.ending_equity)) },
  {
    key: "pnl",
    label: (
      <>
        Net P&amp;L, simulated <Placeholder name="performance" />
      </>
    ),
    cell: (m) => <NetPnl value={m.net_pnl} />,
  },
  { key: "return", label: "Total return", cell: (m) => figure(exactPercent(m.total_return)) },
  { key: "vol", label: "Volatility per period", cell: (m) => <Statistic value={m.volatility} absent={m.absent} format={exactPercent} /> },
  { key: "vol-a", label: "Volatility, annualized", cell: (m) => <Statistic value={m.volatility_annualized} absent={m.absent} format={exactPercent} /> },
  { key: "sharpe", label: "Sharpe ratio per period", cell: (m) => <Statistic value={m.sharpe} absent={m.absent} format={exactNumber} /> },
  { key: "sharpe-a", label: "Sharpe ratio, annualized", cell: (m) => <Statistic value={m.sharpe_annualized} absent={m.absent} format={exactNumber} /> },
  { key: "dd", label: "Maximum drawdown", cell: (m) => figure(exactPercent(m.max_drawdown)) },
  { key: "dd-usd", label: "Maximum drawdown in dollars", cell: (m) => figure(exactUsd(m.max_drawdown_usd)) },
  { key: "turnover", label: "Turnover", cell: (m) => figure(exactNumber(m.turnover)) },
  { key: "fees", label: "Fees", cell: (m) => figure(exactUsd(m.fees_total)) },
  { key: "fills", label: "Fills", cell: (m) => figure(String(m.fill_count)) },
];

function MetricsTable({ report }: { report: Report }) {
  const title = "This version and buy and hold";
  return (
    <Section title={title} id="backtest-metrics">
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <caption className="sr-only">{title}</caption>
          <thead>
            <tr>
              <th scope="col" className={TH}>
                Measure
              </th>
              <th scope="col" className={TH}>
                This version
              </th>
              <th scope="col" className={TH}>
                Buy and hold
              </th>
            </tr>
          </thead>
          <tbody>
            {METRIC_ROWS.map((row) => (
              <tr key={row.key} className="border-t border-border/70">
                <th scope="row" className={`${TD} text-left font-normal`}>
                  {row.label}
                </th>
                <td className={TD}>{row.cell(report.strategy)}</td>
                <td className={TD}>{row.cell(report.benchmark)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <dl className="grid gap-1">
        <dt className="text-sm text-muted-foreground">Excess total return</dt>
        <dd className="font-mono tabular">{exactPercent(report.excess_total_return)}</dd>
      </dl>
    </Section>
  );
}

function CountTable({ caption, head, entries }: { caption: string; head: string; entries: Array<[string, number]> }) {
  return (
    <table className="w-full max-w-md text-sm">
      <caption className="mb-1 text-left text-h3">{caption}</caption>
      <thead>
        <tr>
          <th scope="col" className={TH}>
            {head}
          </th>
          <th scope="col" className={TH}>
            Count
          </th>
        </tr>
      </thead>
      <tbody>
        {entries.length === 0 ? (
          <tr className="border-t border-border/70">
            <td colSpan={2} className={TD}>
              <None />
            </td>
          </tr>
        ) : (
          entries.map(([code, n]) => (
            <tr key={code} className="border-t border-border/70">
              <th scope="row" className={`${TD} text-left font-normal`}>
                <Code>{code}</Code>
              </th>
              <td className={`${TD} font-mono tabular`}>{n}</td>
            </tr>
          ))
        )}
      </tbody>
    </table>
  );
}

const orNone = (value: string | null, show: (v: string) => ReactNode) => (value === null ? <None /> : show(value));

function Decisions({ report }: { report: Report }) {
  const { decisions } = report;
  return (
    <Section title="Decisions">
      <p>{decisions.count === 1 ? "1 decision" : `${decisions.count} decisions`}</p>
      <CountTable caption="Outcomes" head="Outcome" entries={OUTCOMES.map((o) => [o, decisions.outcomes[o]])} />
      <CountTable caption="Gate reasons" head="Reason" entries={Object.entries(decisions.gate_reasons)} />
      <CountTable caption="Hold reasons" head="Reason" entries={Object.entries(decisions.hold_reasons)} />
      <div className="overflow-x-auto">
        <table className="w-full min-w-[48rem] text-sm">
          <caption className="mb-1 text-left text-h3">Each decision</caption>
          <thead>
            <tr>
              {["Date", "Purpose", "Outcome", "Reason", "Decided by", "Quantity", "Limit", "Outputs used"].map((h) => (
                <th key={h} scope="col" className={TH}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {decisions.rows.map((row, i) => (
              <tr key={i} className="border-t border-border/70">
                <td className={`${TD} font-mono tabular`}>{row.date}</td>
                <td className={TD}>{row.purpose === null ? "hold" : <Code>{row.purpose}</Code>}</td>
                <td className={TD}>
                  <Code>{row.outcome}</Code>
                </td>
                <td className={TD}>{orNone(row.reason, (r) => <Code>{r}</Code>)}</td>
                <td className={TD}>{orNone(row.decided_by, (d) => <Code>{d}</Code>)}</td>
                <td className={TD}>{orNone(row.qty, (q) => figure(exactNumber(q)))}</td>
                <td className={TD}>{orNone(row.limit, (l) => figure(exactUsd(l)))}</td>
                <td className={`${TD} font-mono tabular`}>{row.outputs_used}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  );
}

function Risk({ report }: { report: Report }) {
  const { risk } = report;
  return (
    <Section title="Risk">
      <dl className="grid grid-cols-1 gap-x-6 gap-y-4 sm:grid-cols-3">
        <Fact term="Limits latched">
          {risk.limits_latched.length === 0 ? (
            <None />
          ) : (
            <ul className="grid gap-1">
              {risk.limits_latched.map((l) => (
                <li key={l}>
                  <Code>{l}</Code>
                </li>
              ))}
            </ul>
          )}
        </Fact>
        <Fact term="Risk exits">
          <span className="font-mono tabular">{risk.risk_exits}</span>
        </Fact>
        <Fact term="Most restrictive mode">
          <Code>{risk.most_restrictive_mode}</Code>
        </Fact>
      </dl>
    </Section>
  );
}

function Limitations({ report }: { report: Report }) {
  return (
    <Section title="Limitations">
      <ul className="grid gap-1.5">
        {report.limitations.map((l) => (
          <li key={l}>
            <Code>{l}</Code>
          </li>
        ))}
      </ul>
    </Section>
  );
}

function Inputs({ report }: { report: Report }) {
  return (
    <Section title="Inputs">
      <dl className="grid grid-cols-1 gap-x-6 gap-y-4 sm:grid-cols-2">
        <Fact term="Bars">
          <Code>{report.inputs.bars}</Code>
        </Fact>
        <Fact term="Model outputs">
          <Code>{report.inputs.model_outputs}</Code>
        </Fact>
        <Fact term="Theses">
          <Code>{report.inputs.theses}</Code>
        </Fact>
        <Fact term="Configuration">
          <Code>{report.inputs.config}</Code>
        </Fact>
      </dl>
    </Section>
  );
}

/** The legend on every report, first, then each other name the report carries, in its order. */
function disclosuresOf(backtest: Backtest): Disclosure[] {
  return [...new Set<Disclosure>(["LEGEND-HYPOTHETICAL", ...(backtest.report?.disclosures ?? [])])];
}

function TheRun({ backtest }: { backtest: Backtest }) {
  return (
    <Section title="The run">
      <dl className="grid grid-cols-1 gap-x-6 gap-y-4 sm:grid-cols-2">
        <Fact term="State">{STATE[backtest.state]}</Fact>
        <Fact term="Backtest">
          <Code>{backtest.backtest_id}</Code>
        </Fact>
        <Fact term="Mandate version">
          <Code>{backtest.mandate_version}</Code>
        </Fact>
        {backtest.state === "refused" ? (
          <Fact term="Refused with">
            <Code>{backtest.refusal}</Code>
          </Fact>
        ) : null}
        {backtest.state === "completed" ? (
          <>
            <Fact term="Report">
              <Code>{backtest.report_ref}</Code>
            </Fact>
            <Fact term="Instrument">
              <Code>{backtest.report.instrument}</Code>
            </Fact>
          </>
        ) : null}
      </dl>
      {backtest.state === "running" ? <p className={MUTED}>No report yet.</p> : null}
    </Section>
  );
}

/** A decoded backtest, as the owner reads it. */
export function BacktestReport({ backtest }: { backtest: Backtest }) {
  return (
    <article className="grid min-w-0 content-start gap-(--section-gap)" aria-labelledby="backtest-title">
      <header className="grid gap-3">
        <div className="flex flex-wrap items-center gap-3">
          <h1 id="backtest-title" className="text-h1">
            Backtest
          </h1>
          <FixtureTag scope="workspace" />
        </div>
        <div className="flex flex-wrap gap-2">
          {disclosuresOf(backtest).map((name) => (
            <Placeholder key={name} name={PLACEHOLDER[name]} />
          ))}
        </div>
        <p className="max-w-measure text-muted-foreground">Simulated: this mandate version&apos;s own decisions, run over past prices.</p>
      </header>
      <TheRun backtest={backtest} />
      {backtest.state === "completed" ? (
        <>
          <MetricsTable report={backtest.report} />
          <Decisions report={backtest.report} />
          <Risk report={backtest.report} />
          <Limitations report={backtest.report} />
          <Inputs report={backtest.report} />
        </>
      ) : null}
    </article>
  );
}

/** A read that failed: not found, or what could not be read, and nothing else. */
function ReadFailed({ error }: { error: ApiError }) {
  if (error.status === 404 && error.code === "not_found") {
    return (
      <div className="grid gap-3">
        <h1 className="text-h1">Backtest not found</h1>
        <p className={MUTED}>No backtest with this id was found in this workspace.</p>
      </div>
    );
  }
  const issue = error.decode;
  return (
    <div role="alert" className="grid max-w-2xl gap-3">
      <h1 className="text-h1">Backtest could not be read</h1>
      {issue ? (
        <p>
          The answer could not be read: <Code>{issue.problem}</Code> at <Code>{issue.path === "" ? "/" : issue.path}</Code>
          {issue.value !== null ? (
            <>
              , value <Code>{issue.value}</Code>
            </>
          ) : null}
          . Nothing from it is shown.
        </p>
      ) : (
        <p>
          The deployment answered <Code>{error.code}</Code>. Nothing is shown.
        </p>
      )}
    </div>
  );
}

function BacktestLoader({ backtestId }: { backtestId: string }) {
  const { readBacktest } = useRuntime();
  const [shown, setShown] = useState<{ id: string; read: BacktestRead } | null>(null);
  useEffect(() => {
    let live = true;
    void readBacktest(backtestId).then((read) => {
      if (live) setShown({ id: backtestId, read });
    });
    return () => {
      live = false;
    };
  }, [readBacktest, backtestId]);
  const read = shown?.id === backtestId ? shown.read : null;
  if (!read)
    return (
      <>
        <h1 className="sr-only">Backtest</h1>
        <ScreenSkeleton />
      </>
    );
  return read.ok ? <BacktestReport backtest={read.backtest} /> : <ReadFailed error={read.error} />;
}

/** One backtest's report (B2), read through the runtime from either data source (DEC-2400). */
export function BacktestReportScreen({ backtestId }: { backtestId: string }) {
  return (
    <WorkspaceGate api>
      <BacktestLoader backtestId={backtestId} />
    </WorkspaceGate>
  );
}

/**
 * An agent's backtests, on the fixtures only: the API has no route that lists them, so over the API the
 * section keeps the not-available notice and this shows nothing (DEC-2400 item 4).
 */
export function AgentBacktests({ agentId }: { agentId: string }) {
  const { source } = useRuntime();
  if (source === "api") return null;
  const ids = BACKTESTS_BY_AGENT[agentId] ?? [];
  if (ids.length === 0) return <p className={MUTED}>No backtest of this agent&apos;s mandate yet.</p>;
  return (
    <ul className="grid divide-y divide-border/70">
      {ids.map((id) => {
        const read = servedBacktest(servedBacktestFixture(id), id);
        return (
          <li key={id}>
            <Link href={`/backtests/${id}`} className={ROW_LINK}>
              <span className="font-medium">{read.ok ? STATE[read.backtest.state] : "Could not be read"}</span>
              <span className="font-mono text-sm break-all text-muted-foreground">{id}</span>
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
