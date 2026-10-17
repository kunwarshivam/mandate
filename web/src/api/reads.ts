/**
 * The workspace the API runtime holds before its reads answer, or when they cannot (DEC-975, DEC-1096):
 * no agent, request, decision or figure. Over the API the screens read the read models (`read-models.ts`).
 */
import type { Workspace } from "@/fixtures/types";

/** The workspace the screens show before the first read answers, or when none can: no agent, no request, no figure. */
export function emptyWorkspace(status: Workspace["status"], now: string): Workspace {
  const feed = { state: "stale" as const, as_of: now };
  return {
    scenario: "normal",
    status,
    journal: "answers",
    now,
    environment: "paper",
    connection: { connection_id: "", broker: "Alpaca paper", account_equity: "0", day_trading_regime: "intraday_margin" },
    approver_users: 0,
    independent_approval_required: null,
    health: { market_data: feed, broker: feed, deployment: status === "unreachable" ? { state: "down", as_of: now } : feed, relay: feed },
    agents: [],
    approvals: [],
    decisions: [],
    timeline: {},
    external_positions: [],
  };
}
