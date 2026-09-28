import type { ReactNode } from "react";
import { ProvenanceBadge } from "@/components/domain/provenance-badge";
import type { Agent, Mandate } from "@/fixtures/types";
import { dec, mul, add, sub, ONE } from "@/lib/decimal";
import { dateLabel, percent, price, quantity, seconds, usd } from "@/lib/format";

interface Row {
  label: string;
  value: ReactNode;
  path?: string;
}

function goalRows(m: Mandate): Row[] {
  const g = m.goal;
  const capital = dec(m.capital.allocation_usd);
  switch (g.type) {
    case "accumulate": {
      const symbol = m.universe.pinned_instruments.find((i) => i.asset_id === g.instrument)?.symbol ?? "the instrument";
      return [
        { label: "Goal", value: `Build up to ${quantity(g.target_qty)} ${symbol}${g.end_date ? ` by ${dateLabel(g.end_date)}` : ""}`, path: "/goal/target_qty" },
        ...(g.max_avg_price ? [{ label: "Average price at most", value: price(g.max_avg_price), path: "/goal/max_avg_price" }] : []),
        { label: "Spend at most", value: usd(g.max_spend_usd), path: "/goal/max_spend_usd" },
        {
          label: "When complete",
          value: g.on_complete === "hold_protected" ? "Hold, with protection" : g.on_complete === "release" ? "Release positions to you" : "Hold; the ladder is disarmed",
        },
      ];
    }
    case "profit_stop":
      return [
        {
          label: "Profit stop",
          value: `Stops when equity reaches ${usd(mul(capital, add(ONE, dec(g.profit_level))))}, ${percent(g.profit_level, 0)} above capital`,
          path: "/goal/profit_level",
        },
      ];
    case "continuous":
      return [{ label: "Goal", value: g.end_date ? `Runs until ${dateLabel(g.end_date)}` : "Runs until you stop it" }];
    default: {
      const unhandled: never = g;
      throw new Error(`unhandled goal ${JSON.stringify(unhandled)}`);
    }
  }
}

export function MandateSummary({ agent }: { agent: Agent }) {
  const m = agent.mandate;
  const r = m.risk;
  const provenance = (path?: string) => (path ? agent.provenance.find((p) => p.path === path) : undefined);
  const capital = dec(m.capital.allocation_usd);
  const rows: Row[] = [
    ...goalRows(m),
    { label: "Capital", value: usd(m.capital.allocation_usd), path: "/capital/allocation_usd" },
    {
      label: "Lifetime loss limit",
      value: `${percent(m.capital.max_loss_from_allocation, 0)}: floor at ${usd(mul(capital, sub(ONE, dec(m.capital.max_loss_from_allocation))))}`,
      path: "/capital/max_loss_from_allocation",
    },
    { label: "Instruments", value: m.universe.pinned_instruments.map((i) => i.symbol).join(", "), path: "/universe/pinned_instruments" },
    { label: "Largest order", value: usd(r.max_order_usd), path: "/risk/max_order_usd" },
    { label: "Largest position", value: `${usd(r.max_position_usd)} or ${percent(r.max_position_fraction, 0)} of equity`, path: "/risk/max_position_usd" },
    { label: "Daily loss limit", value: `${percent(r.max_daily_loss, 0)} of the day's starting equity, then exits only`, path: "/risk/max_daily_loss" },
    {
      label: "Drawdown ladder",
      value: r.drawdown_ladder.map((rung) => `${percent(rung.at, 0)}: ${rung.action === "scale_sizes" ? `sizes to ${percent(rung.factor ?? "1", 0)}` : rung.action === "exits_only" ? "exits only" : "close and pause"}`).join("; "),
      path: "/risk/drawdown_ladder",
    },
    { label: "Protective stop", value: `${percent(m.protection.stop_distance, 0)} below cost`, path: "/protection/stop_distance" },
    {
      label: "Signal models",
      value: m.behavior.signal_models.map((s) => `${s.id} ${s.version} (weight ${s.weight})`).join(", "),
    },
    {
      label: "Your rules",
      value: m.autonomy.rules.map((rule) => `${rule.id}: ${rule.then === "ask" ? "ask you" : rule.then === "auto" ? "submit without asking" : "never"}`).join("; "),
    },
    { label: "Anything else", value: m.autonomy.default === "ask" ? "Ask you" : m.autonomy.default === "auto" ? "Submit without asking" : "Never", path: "/autonomy/default" },
    {
      label: "Approval window",
      value: `${seconds(m.autonomy.approval.timeout_s)}, then skipped${m.autonomy.approval.two_approver_above_usd ? `; two approvers above ${usd(m.autonomy.approval.two_approver_above_usd)}` : ""}`,
      path: m.autonomy.approval.two_approver_above_usd ? "/autonomy/approval/two_approver_above_usd" : undefined,
    },
    { label: "Environment", value: "Paper", path: "/environment" },
  ];

  return (
    <div className="grid gap-4">
      <blockquote className="grid gap-1 border-l-2 border-foreground pl-3 text-sm">
        <p className="label-caps text-muted-foreground">Your description</p>
        <p>“{m.behavior.description}”</p>
      </blockquote>
      <dl className="@container grid divide-y text-sm">
        {rows.map((row) => {
          const p = provenance(row.path);
          return (
            <div key={row.label} className="grid gap-1 py-2 @sm:grid-cols-[10rem_minmax(0,1fr)] @sm:gap-3">
              <dt className="text-muted-foreground">{row.label}</dt>
              <dd className="grid min-w-0 gap-1 wrap-anywhere">
                <span className="flex flex-wrap items-center gap-2">
                  <span>{row.value}</span>
                  <ProvenanceBadge provenance={p} />
                </span>
                {p?.quote ? <span className="text-caption text-muted-foreground">You said “{p.quote}”</span> : null}
              </dd>
            </div>
          );
        })}
      </dl>
      <p className="text-caption text-muted-foreground">
        Version <span className="font-mono">{agent.mandate_version.slice(0, 19)}…</span>, deployed {dateLabel(agent.deployed_at)}.
      </p>
    </div>
  );
}
