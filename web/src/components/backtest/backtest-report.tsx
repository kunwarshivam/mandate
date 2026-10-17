"use client";

import type { Backtest } from "@/api/backtest";

/** One backtest's report (B2), read through the runtime from either data source (DEC-2400). */
export function BacktestReportScreen({ backtestId }: { backtestId: string }): never {
  void backtestId;
  throw new Error("Unimplemented: E4-5");
}

/** A decoded backtest, as the owner reads it. */
export function BacktestReport({ backtest }: { backtest: Backtest }): never {
  void backtest;
  throw new Error("Unimplemented: E4-5");
}

/** An agent's backtests, listed on the fixtures. */
export function AgentBacktests({ agentId }: { agentId: string }): never {
  void agentId;
  throw new Error("Unimplemented: E4-5");
}
