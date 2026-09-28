import type { Agent, LadderRung } from "@/fixtures/types";
import { type Dec, ONE, ZERO, add, dec, max, min, mul, sub } from "./decimal";
import { percent } from "./format";

/**
 * An agent's limits in dollars, from mandate spec §5.2 to §5.7. Each is derived from the mandate
 * and the runtime state; nothing here is a forecast.
 */

export interface Rail {
  key: string;
  label: string;
  used: Dec;
  cap: Dec;
  /** What happens at the cap, in words. */
  atCap: string;
}

export type LevelKind = "floor" | "rung" | "daily" | "high_water_mark" | "profit_stop";

export interface Level {
  key: string;
  kind: LevelKind;
  label: string;
  at: Dec;
  /** What the agent does when equity reaches this level. */
  action: string;
  reached: boolean;
}

export interface AgentLimits {
  equity: Dec;
  rails: Rail[];
  levels: Level[];
  orderCap: Dec;
  ordersToday: number;
  ordersCap: number;
  sizeFactor: Dec;
  drawdown: Dec;
}

function rungAction(rung: LadderRung): string {
  switch (rung.action) {
    case "scale_sizes":
      return `Sizes scaled to ${percent(rung.factor ?? "1", 0)}`;
    case "exits_only":
      return "Exits only";
    case "flatten_and_pause":
      return "Close positions and pause";
    default: {
      const unhandled: never = rung.action;
      throw new Error(`unhandled rung ${String(unhandled)}`);
    }
  }
}

export function agentLimits(agent: Agent): AgentLimits {
  const { mandate, state } = agent;
  const risk = mandate.risk;
  const E = dec(state.equity);
  const E0 = dec(state.equity_day_start);
  const H = dec(state.high_water_mark);
  const C = dec(state.capital_base);
  const L = dec(state.inherited_loss);

  const positionCap = min(dec(risk.max_position_usd), mul(dec(risk.max_position_fraction), E));
  const grossCap = min(dec(risk.max_gross_exposure_usd), E);
  const dailyBudget = mul(dec(risk.max_daily_loss), E0);
  const lossToday = max(ZERO, sub(E0, E));
  const gross = add(...agent.positions.map((p) => dec(p.market_value)));
  const dailyAction = risk.daily_loss_action === "exits_only" ? "Exits only until a new risk day" : "Close positions and pause";

  const rails: Rail[] = [
    ...agent.positions.map((p) => ({
      key: `position-${p.instrument.symbol}`,
      label: `${p.instrument.symbol} position`,
      used: dec(p.market_value),
      cap: positionCap,
      atCap: "No larger position in it",
    })),
    { key: "gross", label: "Total holdings", used: gross, cap: grossCap, atCap: "No new buys" },
    { key: "daily", label: "Loss today", used: lossToday, cap: dailyBudget, atCap: dailyAction },
  ];

  const levels: Level[] = [
    {
      key: "floor",
      kind: "floor",
      label: "Lifetime floor",
      at: add(mul(C, sub(ONE, dec(mandate.capital.max_loss_from_allocation))), L),
      action: "Close positions and pause for good",
      reached: false,
    },
    ...risk.drawdown_ladder.map((rung, i) => ({
      key: `rung-${i}`,
      kind: "rung" as const,
      label: `Drawdown ${percent(rung.at, 0)}`,
      at: mul(H, sub(ONE, dec(rung.at))),
      action: rungAction(rung),
      reached: false,
    })),
    {
      key: "daily",
      kind: "daily",
      label: "Daily loss limit",
      at: sub(E0, dailyBudget),
      action: dailyAction,
      reached: false,
    },
    { key: "hwm", kind: "high_water_mark", label: "High-water mark", at: H, action: "Drawdown is measured from here", reached: false },
  ];
  if (mandate.goal.type === "profit_stop") {
    levels.push({
      key: "profit-stop",
      kind: "profit_stop",
      label: "Profit stop",
      at: mul(C, add(ONE, dec(mandate.goal.profit_level))),
      action: "The agent stops here",
      reached: false,
    });
  }
  for (const level of levels) {
    level.reached = level.kind === "profit_stop" ? E >= level.at : level.kind === "high_water_mark" ? false : E <= level.at;
  }
  levels.sort((a, b) => (a.at < b.at ? -1 : a.at > b.at ? 1 : 0));

  return {
    equity: E,
    rails,
    levels,
    orderCap: dec(risk.max_order_usd),
    ordersToday: state.orders_today,
    ordersCap: risk.max_orders_per_day,
    sizeFactor: dec(state.size_factor),
    drawdown: max(ZERO, sub(H, E)),
  };
}
