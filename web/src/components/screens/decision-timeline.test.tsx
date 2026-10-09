import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Agent, ContentRef, GateDecision, Workspace } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { VERSION } from "@/fixtures/versions";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { DecisionTimeline } from "./decision-timeline";

/** The swing agent's third version, built here: the owner lowered "low_score" from 0.65 to 0.5. */
const LOWERED: ContentRef = "sha256:0000000000000000000000000000000000000000000000000000000000000c06";

/** A version this agent never had: a decision bound to it cannot be read against any document. */
const UNKNOWN: ContentRef = "sha256:00000000000000000000000000000000000000000000000000000000000dead0";

const LOWERED_AT = "2026-09-28T13:00:00-04:00";

/**
 * The sentences each version's rule reads, written out rather than built from the code, so the test
 * is its own oracle: version 2 asks below 0.65, version 3 below 0.5.
 */
const BEFORE = "Asked you for approval (your rule: ask when the combined model score is below 0.65).";
const AFTER = "Asked you for approval (your rule: ask when the combined model score is below 0.5).";
const KEPT = "Asked you for approval (your rule “low_score”).";

function lowered(ws: Workspace): { ws: Workspace; swing: Agent } {
  const next = structuredClone(ws);
  const swing = next.agents.find((a) => a.agent_id === AGENT_IDS.swing)!;
  const index = swing.mandate.autonomy.rules.findIndex((r) => r.id === "low_score");
  expect(swing.mandate.autonomy.rules[index].when.value).toBe("0.65");
  swing.mandate.autonomy.rules[index] = { ...swing.mandate.autonomy.rules[index], when: { ...swing.mandate.autonomy.rules[index].when, value: "0.5" } };
  swing.versions = [
    ...swing.versions,
    {
      mandate_version: LOWERED,
      previous: VERSION.swing,
      confirmed_at: LOWERED_AT,
      step_up: true,
      classification: "risk_increasing",
      changes: [{ path: `/autonomy/rules/${index}/when/value`, from: "0.65", to: "0.5", classification: "risk_increasing" }],
      application: { result: "applied", at: LOWERED_AT, approvals_canceled: 0 },
    },
  ];
  swing.mandate_version = LOWERED;
  return { ws: next, swing };
}

function asked(eventId: string, at: string, mandateVersion: ContentRef): GateDecision {
  return {
    event_id: eventId,
    at,
    agent_id: AGENT_IDS.swing,
    mandate_version: mandateVersion,
    verdict: "allow",
    reason_code: null,
    action: { side: "buy", qty: "1", symbol: "XYZ", limit_price: "140", purpose: "increase" },
    then: KEPT,
  };
}

function rows(ws: Workspace, decisions: GateDecision[]): HTMLElement[] {
  renderWithRuntime(<DecisionTimeline ws={ws} decisions={decisions} />, "normal", { workspace: () => ws });
  return within(screen.getByRole("list", { name: "Decisions, newest first" })).getAllByRole("listitem");
}

beforeEach(() => setPathname("/"));

describe("a decision reads its rule as the mandate version it was decided under said it (C-6)", () => {
  it("reads the old threshold before the change and the new one after it", () => {
    const { ws } = lowered(buildWorkspace("normal"));
    const [after, before] = rows(ws, [asked("01JBTESTAFTER0000000000000", "2026-09-28T13:30:00-04:00", LOWERED), asked("01JBTESTBEFORE000000000000", "2026-09-28T12:30:00-04:00", VERSION.swing)]);
    expect(before).toHaveTextContent(BEFORE);
    expect(before).not.toHaveTextContent("0.5)");
    expect(after).toHaveTextContent(AFTER);
    expect(after).not.toHaveTextContent("0.65");
  });

  it("reads a decision from the first version against the first version's document", () => {
    const { ws } = lowered(buildWorkspace("normal"));
    const [first] = rows(ws, [asked("01JBTESTFIRST0000000000000", "2026-09-23T10:00:00-04:00", VERSION.swingFirst)]);
    expect(first).toHaveTextContent(BEFORE);
  });

  it("keeps the rule's id when the decision's version is not in the agent's history, never the current rule", () => {
    const { ws } = lowered(buildWorkspace("normal"));
    const [unknown] = rows(ws, [asked("01JBTESTUNKNOWN00000000000", "2026-09-28T13:30:00-04:00", UNKNOWN)]);
    expect(unknown).toHaveTextContent(KEPT);
    expect(unknown).not.toHaveTextContent("0.5");
  });

  it("keeps the rule's id when the decision carries no version at all", () => {
    const { ws } = lowered(buildWorkspace("normal"));
    const unversioned: Partial<GateDecision> = asked("01JBTESTNOVERSION000000000", "2026-09-28T13:30:00-04:00", LOWERED);
    delete unversioned.mandate_version;
    const [missing] = rows(ws, [unversioned as GateDecision]);
    expect(missing).toHaveTextContent(KEPT);
    expect(missing).not.toHaveTextContent("0.5");
  });
});
