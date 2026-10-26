/**
 * Placeholder legal text and the platform policy values counsel sets (compliance doc, "Retail" and
 * "Terms and disclosures"; DEC-102, DEC-2134). None of it is final: agents never write legal text
 * as final (DEC-79), so every document and value here carries {@link PENDING_COUNSEL} wherever it
 * is shown, and none of it may be used for real money. Until counsel signs off every customer
 * trades paper, and the customer real-money gate stays off (DEC-2134).
 */

import { canonicalJson } from "./fixture-journey";
import { sha256Hex } from "./sha256";

/** The mark every placeholder carries, word for word. */
export const PENDING_COUNSEL = "Pending counsel — not for real-money use";

export interface PlaceholderDocument {
  /** The document id a later acceptance names (journal spec §9.2 `DisclosureAccepted`). */
  id: string;
  title: string;
  paragraphs: readonly string[];
}

/**
 * The risk disclosures the go-live screen shows before an agent trades, as the compliance doc
 * lists them: automated trading, model error, venue and connectivity, and leverage. Placeholders
 * that name the risk without legal wording; counsel writes the text.
 */
export const RISK_DISCLOSURES: readonly PlaceholderDocument[] = [
  {
    id: "risk_automated_trading",
    title: "Automated trading",
    paragraphs: [
      "Placeholder. Your agent places orders in your account without asking you each time, within the limits you confirm. Orders it places can lose money.",
      "Placeholder. Paper trading uses simulated money. Results on paper do not show what would happen with real money.",
    ],
  },
  {
    id: "risk_model_error",
    title: "Model error",
    paragraphs: ["Placeholder. The models your agent reads can be wrong, late, or unavailable. A score is not a probability of profit."],
  },
  {
    id: "risk_venue_connectivity",
    title: "Broker and connectivity",
    paragraphs: ["Placeholder. Your broker, market data, or network can fail or be delayed, and an order can be filled at a worse price than shown."],
  },
  {
    id: "risk_leverage",
    title: "Leverage",
    paragraphs: ["Placeholder. Agents trade at one times your equity with no borrowing, and leveraged products are off. Counsel will set what live trading allows."],
  },
];

export interface PendingValue {
  label: string;
  /** The value in force for the demo: the most conservative one the specs already name. */
  value: string;
  /** Where that value comes from. */
  source: string;
}

/**
 * The platform policy values counsel sets, at the conservative placeholders the mandate spec
 * names until counsel answers (mandate spec §9's retail profile, DEC-61, DEC-98; DEC-22 for the
 * jurisdiction).
 */
export const PENDING_POLICY: readonly PendingValue[] = [
  { label: "Lifetime-loss ceiling", value: "At most 20% of an agent's allocation", source: "Retail profile placeholder (DEC-61)" },
  { label: "Approval timeout, minimum", value: "At least 120 seconds", source: "Retail profile placeholder (DEC-61)" },
  { label: "Allowed jurisdictions", value: "United States only", source: "DEC-22" },
  { label: "Environments", value: "Paper only", source: "Retail profile (DEC-98); real money off (DEC-2134)" },
];

/** The go-live disclosure as the workspace stores it (`fixtures/disclosures/go_live_risk.json`), which a test holds equal to the file. */
export const GO_LIVE_RISK = { document: "go_live_risk", status: PENDING_COUNSEL, sections: RISK_DISCLOSURES } as const;

/** Its version: `sha256:` over its canonical JSON, the value `DisclosureAccepted` names when the owner accepts it on the host. */
export const GO_LIVE_RISK_VERSION = `sha256:${sha256Hex(canonicalJson(GO_LIVE_RISK))}`;

/** The command the owner runs on the host to accept it; without `--code` it shows the code, which the owner then passes. */
export function acceptCommand(workspaceId: string, principalId: string): string {
  return `mandate disclosure accept ${GO_LIVE_RISK.document} --workspace ${workspaceId} --user ${principalId} --journal "$MANDATE_JOURNAL_DSN" --store <artifact store>`;
}
