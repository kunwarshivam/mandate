import { screen } from "@testing-library/react";
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
  return [...screen.getByRole("list", { name: "Decisions, newest first" }).querySelectorAll<HTMLElement>("[data-slot=timeline-entry]")];
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

/** The runtime's day in these tests: the fixture's now is 28 September 2026, 14:05:20 ET. */
const TODAY = "2026-09-28";

const ONE_DAY_EACH = (): GateDecision[] => [asked("01JBC19TODAY00000000000000", `${TODAY}T13:30:00-04:00`, VERSION.swing), asked("01JBC19OLDER00000000000000", "2026-09-25T08:15:00-04:00", VERSION.swing)];

function timeline(decisions: GateDecision[], now?: string) {
  const ws = buildWorkspace("normal");
  renderWithRuntime(<DecisionTimeline ws={ws} decisions={decisions} />, "normal", { workspace: () => (now ? { ...ws, now } : ws) });
}

function headings(): (string | null)[] {
  return screen.queryAllByRole("heading", { level: 3 }).map((h) => h.textContent);
}

/** The entries under a day's heading, found by the list that heading names. */
function underHeading(day: string): HTMLElement[] {
  return [...screen.getByRole("list", { name: day }).querySelectorAll<HTMLElement>("[data-slot=timeline-entry]")];
}

/** What an entry's time shows on screen: its text without the full date kept for assistive tech. */
function shownTime(entry: HTMLElement): string {
  const time = entry.querySelector("time")!.cloneNode(true) as HTMLElement;
  time.querySelectorAll("[data-slot=full-date]").forEach((n) => n.remove());
  return time.textContent ?? "";
}

describe("the timeline marks where each day ends (C-19)", () => {
  it("puts entries from two days under two headings, Today and the older date, newest first", () => {
    timeline(ONE_DAY_EACH());
    expect(headings()).toEqual(["Today", "25 September"]);
    expect(underHeading("Today")).toHaveLength(1);
    expect(underHeading("25 September")).toHaveLength(1);
  });

  it("shows times alone under each heading", () => {
    timeline(ONE_DAY_EACH());
    expect(underHeading("Today").map(shownTime)).toEqual(["13:30"]);
    expect(underHeading("25 September").map(shownTime)).toEqual(["08:15"]);
  });

  it("still gives each entry its full date and time for assistive tech", () => {
    timeline(ONE_DAY_EACH());
    const [today] = underHeading("Today");
    const [older] = underHeading("25 September");
    expect(older.querySelector("time")).toHaveAttribute("dateTime", "2026-09-25T08:15:00-04:00");
    expect(older.querySelector("time")).toHaveTextContent(/^Sep 25, 2026, 08:15$/);
    expect(today.querySelector("time")).toHaveTextContent(/^Sep 28, 2026, 13:30$/);
  });

  it("puts an entry exactly at midnight under the day it starts, and one a second before under the day before", () => {
    timeline([
      asked("01JBC19MIDNIGHTUTC00000000", `${TODAY}T04:00:00Z`, VERSION.swing),
      asked("01JBC19MIDNIGHT00000000000", `${TODAY}T00:00:00-04:00`, VERSION.swing),
      asked("01JBC19BEFOREMIDNIGHT00000", "2026-09-27T23:59:59-04:00", VERSION.swing),
    ]);
    expect(headings()).toEqual(["Today", "27 September"]);
    expect(underHeading("Today").map(shownTime)).toEqual(["00:00", "00:00"]);
    expect(underHeading("27 September").map(shownTime)).toEqual(["23:59"]);
  });

  it("takes Today from the runtime's now, never the machine's clock", () => {
    timeline([asked("01JBC19OLDER00000000000000", "2026-09-25T08:15:00-04:00", VERSION.swing)], "2026-09-25T09:00:00-04:00");
    expect(headings()).toEqual(["Today"]);
  });

  it("adds the year only to a day from another year", () => {
    timeline([asked("01JBC19JANUARY000000000000", "2026-01-02T10:05:00-05:00", VERSION.swing), asked("01JBC19LASTYEAR00000000000", "2025-12-31T16:00:00-05:00", VERSION.swing)]);
    expect(headings()).toEqual(["2 January", "31 December 2025"]);
  });

  it("shows no headings when there is nothing to show", () => {
    timeline([]);
    expect(screen.queryAllByRole("heading")).toEqual([]);
  });
});
