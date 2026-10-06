import type { Agent, LadderRung } from "@/fixtures/types";
import { type Dec, ONE, ZERO, add, dec, max, min, mul, ratio, sub } from "./decimal";
import { percent, usd } from "./format";

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
  /**
   * The loss the limit allows, in dollars, from where it is measured down to the level: the day's
   * loss budget, a rung's depth below the high-water mark, or the capital the floor lets go. Null for
   * levels that are not loss limits.
   */
  allowance: Dec | null;
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

  const floor = add(mul(C, sub(ONE, dec(mandate.capital.max_loss_from_allocation))), L);
  const levels: Level[] = [
    {
      key: "floor",
      kind: "floor",
      label: "Lifetime floor",
      at: floor,
      action: "Close positions and pause for good",
      reached: false,
      allowance: sub(C, floor),
    },
    ...risk.drawdown_ladder.map((rung, i) => ({
      key: `rung-${i}`,
      kind: "rung" as const,
      label: `Drawdown ${percent(rung.at, 0)}`,
      at: mul(H, sub(ONE, dec(rung.at))),
      action: rungAction(rung),
      reached: false,
      allowance: mul(H, dec(rung.at)),
    })),
    {
      key: "daily",
      kind: "daily",
      label: "Daily loss limit",
      at: sub(E0, dailyBudget),
      action: dailyAction,
      reached: false,
      allowance: dailyBudget,
    },
    { key: "hwm", kind: "high_water_mark", label: "High-water mark", at: H, action: "Drawdown is measured from here", reached: false, allowance: null },
  ];
  if (mandate.goal.type === "profit_stop") {
    levels.push({
      key: "profit-stop",
      kind: "profit_stop",
      label: "Profit stop",
      at: mul(C, add(ONE, dec(mandate.goal.profit_level))),
      action: "The agent stops here",
      reached: false,
      allowance: null,
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

export interface NextLevel {
  level: Level;
  /** How far equity is from the level, in dollars. */
  distance: Dec;
  side: "below" | "above";
}

/**
 * The next level where the agent's behaviour changes: the closest one below equity not yet reached,
 * or, with none left below, a profit stop above. The high-water mark changes nothing, so it is skipped.
 */
export function nextLevel(limits: AgentLimits): NextLevel | null {
  const E = limits.equity;
  const below = limits.levels.filter((l) => !l.reached && l.kind !== "high_water_mark" && l.kind !== "profit_stop" && l.at < E);
  if (below.length > 0) {
    const level = below.reduce((a, b) => (b.at > a.at ? b : a));
    return { level, distance: headroomAbove(limits, level), side: "below" };
  }
  const stop = limits.levels.find((l) => l.kind === "profit_stop" && !l.reached);
  return stop ? { level: stop, distance: sub(stop.at, E), side: "above" } : null;
}

/** How far equity sits above a level, in dollars, as the rails and the headroom rows show it. */
export function headroomAbove(limits: AgentLimits, level: Level): Dec {
  return sub(limits.equity, level.at);
}

/** A level named in running text: "daily loss limit", "drawdown 10% level", "lifetime floor". */
export function levelNoun(level: Level): string {
  switch (level.kind) {
    case "floor":
      return "lifetime floor";
    case "rung":
      return `${level.label.toLowerCase()} level`;
    case "daily":
      return "daily loss limit";
    case "high_water_mark":
      return "high-water mark";
    case "profit_stop":
      return "profit stop";
    default: {
      const unhandled: never = level.kind;
      throw new Error(`unhandled level ${String(unhandled)}`);
    }
  }
}

/**
 * An agent's headroom in one line, from the next level where its behaviour changes: "$274.45 above
 * its daily loss limit". A distance to a limit, never a result, so it needs no disclosure.
 */
export function headroomLine(agent: Agent): string {
  const next = nextLevel(agentLimits(agent));
  if (!next) return "No limit level ahead";
  return `${usd(next.distance)} ${next.side === "below" ? "above" : "below"} its ${levelNoun(next.level)}`;
}

/** An agent is near a loss limit once it has used this share of the limit's allowance (DEC-206). */
export const NEAR_LIMIT_USED = dec("0.8");

/**
 * The loss limits an agent is near, deepest first: those whose headroom (equity above the level, as
 * the rails show it) is no more than a fifth of the loss the limit allows, reached ones included.
 */
export function nearLossLimits(limits: AgentLimits): Level[] {
  return limits.levels.filter((l) => l.allowance !== null && headroomAbove(limits, l) <= mul(sub(ONE, NEAR_LIMIT_USED), max(ZERO, l.allowance)));
}

export interface HeadroomRow {
  key: string;
  label: string;
  headroom: string;
  limit: string;
  share: number;
  over: boolean;
  atCap: string;
}

/**
 * Each limit as the room left under it. Room under the daily loss limit is equity's distance to that
 * level, the figure Home states, so a day's gain widens it; the other limits are the cap less what is
 * used. Distances to limits, never results, so they need no disclosure.
 */
export function headroomRows(limits: AgentLimits): HeadroomRow[] {
  const daily = limits.levels.find((l) => l.kind === "daily");
  return limits.rails.map((rail) => {
    const left = rail.key === "daily" && daily ? headroomAbove(limits, daily) : sub(rail.cap, rail.used);
    return {
      key: rail.key,
      label: rail.key === "daily" ? "Daily loss limit" : rail.label,
      headroom: usd(left > 0n ? left : 0n),
      limit: rail.key === "daily" ? `${usd(rail.cap)} below the day's start` : usd(rail.cap),
      share: Math.min(ratio(rail.used, rail.cap), 1),
      over: rail.used > rail.cap,
      atCap: rail.atCap,
    };
  });
}
