import type { ActiveRestriction, AgentMode, RestrictionCode } from "@/fixtures/types";

/**
 * The restriction explainer of the product-experience brief §4.3. The mode banner, the dashboard,
 * and alerts read this one table, so every restriction says what it blocks, how it ends, and who
 * can end it.
 */
/**
 * Who imposed a restriction decides its colour (web/DESIGN.md): your mandate is azure, the
 * account is the account colour, your own stop is ink, and market data carries no meaning colour.
 */
export type RestrictionSource = "mandate" | "account" | "owner" | "market";

export interface RestrictionText {
  label: string;
  source: RestrictionSource;
  /** The effective mode the restriction imposes; null when it narrows without a mode change. */
  mode: AgentMode | null;
  blocks: string;
  endsWhen: string;
  /**
   * Who ends it and, when the owner does, where (critique C-24). A place that is not built yet is
   * named as coming in the next slice, with no door to it.
   */
  whoActs: string;
  /** The owner ends it in the Stop sheet, behind a passkey, so the banner offers to open Stop. */
  endsInStop?: true;
}

export const RESTRICTIONS: Record<RestrictionCode, RestrictionText> = {
  drawdown_scale_sizes: {
    label: "Drawdown: sizes scaled",
    source: "mandate",
    mode: null,
    blocks: "Full-size buys",
    endsWhen: "Drawdown recovers past the hysteresis and stays there for the scale-lift time",
    whoActs: "Automatic",
  },
  drawdown_exits_only: {
    label: "Drawdown: selling only",
    source: "mandate",
    mode: "exits_only",
    blocks: "Openings and increases",
    endsWhen: "You acknowledge it with step-up, which resets the high-water mark",
    whoActs: "You, on this agent's Overview. Acknowledging there comes in the next slice.",
  },
  drawdown_flatten: {
    label: "Drawdown: flattened",
    source: "mandate",
    mode: "paused",
    blocks: "Every new order except protection",
    endsWhen: "You acknowledge it once the agent is flat",
    whoActs: "You, on this agent's Overview. Acknowledging there comes in the next slice.",
  },
  daily_loss: {
    label: "Daily loss limit",
    source: "mandate",
    mode: "exits_only",
    blocks: "Openings and increases (every new order except protection after a flatten)",
    endsWhen: "A new risk day plus the minimum time; after a flatten, also your acknowledgment once flat",
    whoActs: "Automatic, then you on this agent's Overview. Acknowledging there comes in the next slice.",
  },
  hard_breach: {
    label: "Hard limit reached",
    source: "mandate",
    mode: "exits_only",
    blocks: "Openings and increases",
    endsWhen: "A sane quote below the hard level, or the limit latches",
    whoActs: "Automatic",
  },
  lifetime_floor: {
    label: "Lifetime floor",
    source: "mandate",
    mode: "paused",
    blocks: "Every new order except protection",
    endsWhen: "Only a mandate version that loosens the floor, with independent approval",
    whoActs: "You and a second user",
  },
  goal_complete: {
    label: "Goal complete",
    source: "mandate",
    mode: "exits_only",
    blocks: "Openings and increases",
    endsWhen: "You release or close the positions",
    whoActs: "You, in Stop",
    endsInStop: true,
  },
  external_activity: {
    label: "Activity outside Owlhead on the account",
    source: "account",
    mode: "exits_only",
    blocks: "Openings and increases on the account",
    endsWhen: "You acknowledge the activity",
    whoActs: "You, on the account's page in Connections. That page comes in the next slice.",
  },
  account_closing_only: {
    label: "Account closing only",
    source: "account",
    mode: "exits_only",
    blocks: "Openings and increases",
    endsWhen: "You acknowledge it and the account refreshes",
    whoActs: "You, on the account's page in Connections. That page comes in the next slice.",
  },
  account_blocked: {
    label: "Account blocked",
    source: "account",
    mode: "paused",
    blocks: "Every new order except protection",
    endsWhen: "You acknowledge it and the account refreshes",
    whoActs: "You, on the account's page in Connections. That page comes in the next slice.",
  },
  reconciliation_mismatch: {
    label: "Ledger and broker disagree",
    source: "account",
    mode: "paused",
    blocks: "Every new order except protection",
    endsWhen: "You review the difference and acknowledge it with step-up",
    whoActs: "You, on the account's page in Connections. That page comes in the next slice.",
  },
  startup_reconciliation: {
    label: "Checking with the broker",
    source: "account",
    mode: "paused",
    blocks: "Every new order except protection",
    endsWhen: "The reconciliation run confirms the ledger matches the broker",
    whoActs: "Automatic",
  },
  leverage_check_failed: {
    label: "Account is not at 1×",
    source: "account",
    mode: "paused",
    blocks: "Every new order except protection",
    endsWhen: "The account is set to 1× at the broker",
    whoActs: "You, at the broker",
  },
  owner_pause: {
    label: "Paused by you",
    source: "owner",
    mode: "paused",
    blocks: "Every new order except protection",
    endsWhen: "You resume the agent, with step-up",
    whoActs: "You, in Stop",
    endsInStop: true,
  },
  stopped: {
    label: "Stopped",
    source: "owner",
    mode: "stopped",
    blocks: "Everything",
    endsWhen: "Never; stopping is final",
    whoActs: "Nobody",
  },
  stale_mark: {
    label: "Stale price",
    source: "market",
    mode: null,
    blocks: "Openings in this instrument",
    endsWhen: "A sane price arrives",
    whoActs: "Automatic",
  },
  removed_instrument: {
    label: "Instrument removed",
    source: "mandate",
    mode: null,
    blocks: "Openings in this instrument",
    endsWhen: "A version re-adds it",
    whoActs: "You, in a new mandate version. Changing instruments comes in the next slice.",
  },
};

export function describeRestriction(r: ActiveRestriction): RestrictionText & { title: string } {
  const text = RESTRICTIONS[r.code];
  return { ...text, title: r.symbol ? `${text.label}: ${r.symbol}` : text.label };
}

export const SOURCE_LABEL: Record<RestrictionSource, string> = {
  mandate: "Your mandate",
  account: "The account",
  owner: "You",
  market: "Market data",
};
