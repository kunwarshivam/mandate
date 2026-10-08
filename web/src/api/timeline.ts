/**
 * An agent's timeline on the workspace API (E11-9; workspace API spec §4.8, J1): `GET
 * /agents/{id}/timeline`, decoded strictly. Gate entries carry the decision's summary, which fills
 * D1's and D2's decision timeline; the full per-check record (J6) is lane L5's, reached through a
 * seam that answers "not available yet" until its schema lands (DEC-737).
 */
import type { GateDecision, Purpose, TimelineEvent, TimelineKind } from "@/fixtures/types";
import type { WorkspaceClient } from "./client";
import type { Decoder, Timestamp } from "./types";

export interface GateSummary {
  verdict: GateDecision["verdict"];
  reason_code: string | null;
  action: { side: "buy" | "sell"; qty: string; symbol: string; limit_price: string; purpose: Purpose };
}

export type TimelineEntry =
  | { kind: "gate"; event_id: string; stream_id: string; seq: number; recorded_at: Timestamp; event_type: "GateDecided"; text: string; gate: GateSummary }
  | { kind: Exclude<TimelineKind, "gate">; event_id: string; stream_id: string; seq: number; recorded_at: Timestamp; event_type: string; text: string };

export interface TimelineReadModel {
  served_at: Timestamp;
  events: TimelineEntry[];
  cursors: Array<{ stream_id: string; after_seq: number }>;
  complete: boolean;
}

/** J6's per-check record, once lane L5 serves it; until then, `not_available`. */
export type GateDetail = { kind: "not_available" } | { kind: "detail"; event_id: string; record: unknown };

export interface GateDetailSource {
  detail(eventId: string): Promise<GateDetail>;
}

export const decodeTimeline: Decoder<TimelineReadModel> = () => {
  throw new Error("Unimplemented: E11-9");
};

/** The timeline as the screens' existing `TimelineEvent`s, in the order served. */
export function timelineEvents(timeline: TimelineReadModel): TimelineEvent[] {
  void timeline;
  throw new Error("Unimplemented: E11-9");
}

/** The gate entries as the screens' existing `GateDecision`s; what an allow led to is the full record's, so it is absent. */
export function gateDecisions(timeline: TimelineReadModel, agentId: string): GateDecision[] {
  void timeline;
  void agentId;
  throw new Error("Unimplemented: E11-9");
}

/** The J6 detail seam. Until lane L5's audit schema lands, it answers `not_available` and reads nothing. */
export function gateDetailSource(client: WorkspaceClient): GateDetailSource {
  void client;
  throw new Error("Unimplemented: E11-9");
}
