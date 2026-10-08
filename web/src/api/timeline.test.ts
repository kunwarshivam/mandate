import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { GateDecision } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { createWorkspaceClient, type Fetch } from "./client";
import { decodeTimeline, gateDecisions, gateDetailSource, timelineEvents } from "./timeline";

const WS = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const SERVED = "2026-09-28T18:05:20.000000000Z";
const HASH = `sha256:${"0f".repeat(32)}` as const;

function example(name: string): Record<string, unknown> {
  const examples = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "schemas", "workspace-api", "examples");
  return JSON.parse(readFileSync(join(examples, `read-models.${name}.json`), "utf8")) as Record<string, unknown>;
}

const canonical = (iso: string) => `${new Date(iso).toISOString().slice(0, 23)}000000Z`;

/** A fixture gate decision as a timeline gate entry, written independently of the adapter. */
function entryOf(d: GateDecision, seq: number) {
  return {
    event_id: d.event_id,
    stream_id: `agent:${WS}:${d.agent_id}`,
    seq,
    recorded_at: canonical(d.at),
    event_type: "GateDecided",
    kind: "gate",
    text: "The gate decided.",
    gate: { verdict: d.verdict, reason_code: d.reason_code, action: d.action },
  };
}

function timelineOf(entries: unknown[]) {
  return { api_version: "v1", build: HASH, served_at: SERVED, as_of: [{ stream_id: `ctl:${WS}`, seq: 1, hash: HASH, recorded_at: SERVED }], events: entries, cursors: [], complete: true };
}

function decoded(body: unknown) {
  const d = decodeTimeline(body, "");
  if (!d.ok) throw new Error(`timeline did not decode: ${JSON.stringify(d.issue)}`);
  return d.value;
}

describe("decoding the timeline (§4.8)", () => {
  it.skip("pending E11-9: the schema's own example decodes", () => {
    expect(decodeTimeline(example("timeline"), "").ok).toBe(true);
  });

  it.skip("pending E11-9: a gate entry without its summary is refused", () => {
    const body = example("timeline");
    const events = structuredClone(body.events) as Array<Record<string, unknown>>;
    const gate = events.findIndex((e) => e.kind === "gate");
    delete events[gate].gate;
    expect(decodeTimeline({ ...body, events }, "")).toMatchObject({ ok: false, issue: { path: `/events/${gate}/gate`, problem: "missing" } });
  });

  it.skip("pending E11-9: a verdict outside the closed set is refused", () => {
    const body = example("timeline");
    const events = structuredClone(body.events) as Array<Record<string, { verdict?: string }>>;
    const gate = events.findIndex((e) => (e as unknown as { kind: string }).kind === "gate");
    events[gate].gate.verdict = "approve";
    expect(decodeTimeline({ ...body, events }, "")).toMatchObject({ ok: false, issue: { problem: "unknown_enum_value", value: "approve" } });
  });

  it.skip("pending E11-9: an allow with a reason code, or a deny without one, is refused", () => {
    const d = buildWorkspace("normal").decisions;
    const allow = d.find((x) => x.verdict === "allow")!;
    const deny = d.find((x) => x.verdict === "deny")!;
    expect(decodeTimeline(timelineOf([{ ...entryOf(allow, 1), gate: { ...entryOf(allow, 1).gate, reason_code: "max_order_size" } }]), "").ok).toBe(false);
    expect(decodeTimeline(timelineOf([{ ...entryOf(deny, 1), gate: { ...entryOf(deny, 1).gate, reason_code: null } }]), "").ok).toBe(false);
  });

  it.skip("pending E11-9: a kind outside the closed set is refused", () => {
    const body = example("timeline");
    const events = structuredClone(body.events) as Array<Record<string, unknown>>;
    events[0].kind = "chat";
    expect(decodeTimeline({ ...body, events }, "").ok).toBe(false);
  });
});

describe("mapping onto the existing view types (§4.9)", () => {
  it.skip("pending E11-9: every fixture gate decision round-trips its summary", () => {
    const ws = buildWorkspace("normal");
    const timeline = decoded(timelineOf(ws.decisions.map(entryOf)));
    for (const agent of ws.agents) {
      const expected = ws.decisions
        .filter((d) => d.agent_id === agent.agent_id)
        .map((d) => ({ event_id: d.event_id, at: canonical(d.at), agent_id: d.agent_id, verdict: d.verdict, reason_code: d.reason_code, action: d.action }));
      const ours = timeline.events.filter((e) => e.stream_id === `agent:${WS}:${agent.agent_id}`);
      expect(gateDecisions({ ...timeline, events: ours }, agent.agent_id)).toEqual(expected);
    }
  });

  it.skip("pending E11-9: only gate entries become decisions", () => {
    expect(gateDecisions(decoded(example("timeline")), "agt_01JB3K9P2H6SD4F8G1E3W7XYZB").map((d) => d.event_id)).toEqual(["01JBM9S2Y8E7D6C5B4A3Z2Y1X0"]);
  });

  it.skip("pending E11-9: every entry becomes a TimelineEvent in the order served", () => {
    const body = example("timeline");
    const events = body.events as Array<{ event_id: string; recorded_at: string; kind: string; text: string }>;
    expect(timelineEvents(decoded(body))).toEqual(events.map((e) => ({ event_id: e.event_id, at: e.recorded_at, kind: e.kind, text: e.text })));
  });
});

describe("the J6 detail seam (lane L5)", () => {
  it.skip("pending E11-9: answers not available yet, and reads nothing", async () => {
    let reads = 0;
    const fetch: Fetch = async () => {
      reads += 1;
      return new Response("{}", { status: 200 });
    };
    const source = gateDetailSource(createWorkspaceClient({ workspaceId: WS, fetch }));
    expect(await source.detail("01JBM9S2Y8E7D6C5B4A3Z2Y1X0")).toEqual({ kind: "not_available" });
    expect(reads).toBe(0);
  });
});
