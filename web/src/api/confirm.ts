/**
 * Confirming a mandate version (workspace API spec §5.1, §4.1; `commands/confirm-request` and
 * `commands/confirm-accepted`; DEC-2401). The confirm's 202 is decoded exactly as its schema states
 * it, and the step-up a risk-increasing confirm needs is bound to `{mandate_version, agent_id,
 * base_version}`.
 */
import type { ContentRef, Decoder, Watermark } from "./types";

/** Mandate spec §9.2's classification of a version against its base, as §5.1 names it. Closed. */
export const CLASSIFICATIONS = ["risk_increasing", "risk_reducing", "neutral"] as const;
export type Classification = (typeof CLASSIFICATIONS)[number];

/** When a confirmed version applies (mandate spec §2.2). Closed. */
export const APPLIES = ["now", "next_safe_point"] as const;
export type Applies = (typeof APPLIES)[number];

/** The 202 of a confirm (§5.1): the `MandateConfirmed` event, the server's classification, and when it applies. */
export interface ConfirmAccepted {
  command_id: string;
  phase: "recorded";
  classification: Classification;
  applies: Applies;
  as_of: Watermark[];
}

export const confirmAccepted: Decoder<ConfirmAccepted> = (value, path) => {
  void value;
  void path;
  throw new Error("Unimplemented: E10-11");
};

/** The digest a confirm's step-up is bound to: SHA-256 of the canonical `{mandate_version, agent_id, base_version}` (§5.1, §3.6). */
export function confirmStepUpDigest(version: ContentRef, agentId: string | null, base: ContentRef | null): ContentRef {
  void version;
  void agentId;
  void base;
  throw new Error("Unimplemented: E10-11");
}
