"use client";

import type { ReactNode } from "react";
import type { DashboardState } from "@/api/dashboard";

/**
 * D1 on the workspace API (E11-1, E11-9): the dashboard's summary, read from `GET /dashboard` and
 * `GET /health` (spec §4.7). It shows what a summary carries: each agent's label, mode,
 * restrictions, P&L with the paper note, and its marks' age. Headroom, the equity chart, holdings and
 * the decision timeline need the full agent and arrive with S3 (DEC-736). Unreachable shows the same
 * notice as the fixtures, with no agent data.
 */
export function ApiDashboard({ state }: { state: DashboardState }): ReactNode {
  void state;
  throw new Error("Unimplemented: E11-1");
}
