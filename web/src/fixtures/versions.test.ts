// @vitest-environment node
import { describe, expect, it } from "vitest";
import { dec } from "@/lib/decimal";
import type { Agent, ChangeClass, MandateChange, MandateVersionRecord, Workspace } from "./types";
import { SCENARIOS, buildWorkspace } from "./workspace";

type Value = MandateChange["from"];

/** Mandate spec §9.2 "Maximums": larger is increasing, and null means unbounded. */
const MAXIMUMS = new Set(["/capital/allocation_usd", "/risk/max_position_usd", "/risk/max_gross_exposure_usd", "/risk/max_order_usd"]);

const larger = (from: Value, to: Value) => (to === null ? from !== null : from !== null && dec(String(to)) > dec(String(from)));

/**
 * Only the §9.2 rows these fixtures use, read from the spec's table rather than from the fixture.
 * A path with no row here fails, so a new fixture change brings its rule with it.
 */
function oracle({ path, from, to }: MandateChange): ChangeClass {
  if (path.startsWith("/notifications/quiet_hours/")) return "neutral";
  if (path === "/autonomy/approval/two_approver_above_usd") return to !== null && (from === null || dec(String(to)) < dec(String(from))) ? "risk_reducing" : "risk_increasing";
  if (MAXIMUMS.has(path)) return larger(from, to) ? "risk_increasing" : "risk_reducing";
  throw new Error(`no §9.2 row in this oracle for ${path}`);
}

function overall(changes: MandateChange[]): ChangeClass {
  if (changes.some((c) => c.classification === "risk_increasing")) return "risk_increasing";
  if (changes.some((c) => c.classification === "risk_reducing")) return "risk_reducing";
  return "neutral";
}

function at(doc: unknown, pointer: string): unknown {
  return pointer
    .split("/")
    .slice(1)
    .reduce<unknown>((node, key) => (node as Record<string, unknown> | null)?.[key], doc);
}

function inEffect(a: Agent, iso: string): MandateVersionRecord | undefined {
  return a.versions.filter((v) => v.application.result === "applied" && Date.parse(v.application.at) <= Date.parse(iso)).at(-1);
}

const cases = SCENARIOS.flatMap(({ id }) => {
  const ws: Workspace = buildWorkspace(id);
  return ws.agents.map((a) => [id, a.label, a, ws] as const);
});

describe.each(cases)("%s: %s's versions", (_scenario, _label, a, ws) => {
  it("start with the deployed version, confirmed with a passkey, and carry unique hashes", () => {
    const [first] = a.versions;
    expect(first).toMatchObject({ previous: null, classification: null, changes: [], step_up: true, application: { result: "applied", at: a.deployed_at, approvals_canceled: 0 } });
    expect(Date.parse(first.confirmed_at)).toBeLessThanOrEqual(Date.parse(a.deployed_at));
    expect(new Set(a.versions.map((v) => v.mandate_version)).size).toBe(a.versions.length);
    expect(a.versions.map((v) => v.confirmed_at)).toEqual([...a.versions.map((v) => v.confirmed_at)].sort((x, y) => Date.parse(x) - Date.parse(y)));
  });

  it("classify every changed path by §9.2 and the version by its riskiest path, with a passkey exactly when it raises risk", () => {
    for (const v of a.versions.slice(1)) {
      expect(v.changes.length).toBeGreaterThan(0);
      expect(v.changes.map((c) => c.path)).toEqual([...new Set(v.changes.map((c) => c.path))].sort());
      for (const c of v.changes) {
        expect(c.from, c.path).not.toEqual(c.to);
        expect(c.classification, c.path).toBe(oracle(c));
      }
      expect(v.classification).toBe(overall(v.changes));
      expect(v.step_up).toBe(v.classification === "risk_increasing");
    }
  });

  it("apply a reducing or neutral version when confirmed and a risk-increasing one at a later safe point (§2.2)", () => {
    for (const v of a.versions.slice(1)) {
      const lag = Date.parse(v.application.at) - Date.parse(v.confirmed_at);
      if (v.classification === "risk_increasing") expect(lag).toBeGreaterThan(0);
      else expect(v.application).toMatchObject({ result: "applied", at: v.confirmed_at });
    }
  });

  it("diff each version against the one in effect, and end at the agent's mandate and version", () => {
    const value = new Map<string, Value>();
    let current = a.versions[0].mandate_version;
    for (const v of a.versions.slice(1)) {
      expect(v.previous).toBe(current);
      for (const c of v.changes) if (value.has(c.path)) expect(c.from, c.path).toEqual(value.get(c.path));
      if (v.application.result !== "applied") continue;
      for (const c of v.changes) value.set(c.path, c.to);
      current = v.mandate_version;
    }
    expect(current).toBe(a.mandate_version);
    for (const [path, v] of value) expect(at(a.mandate, path), path).toEqual(v);
    for (const v of a.versions.slice(1).filter((x) => x.application.result === "rejected")) {
      for (const c of v.changes) expect(at(a.mandate, c.path), c.path).toEqual(c.from);
    }
  });

  it("bind every approval to the version in effect when it was requested, and count the ones each version canceled", () => {
    const mine = ws.approvals.filter((x) => x.agent_id === a.agent_id);
    for (const approval of mine) expect(approval.bound.mandate_version, approval.approval_id).toBe(inEffect(a, approval.requested_at)?.mandate_version);
    for (const v of a.versions.slice(1)) {
      if (v.application.result !== "applied") continue;
      const canceled = mine.filter((x) => x.resolution?.cancel_reason === "version_applied" && x.resolution.at === v.application.at);
      expect(canceled).toHaveLength(v.application.approvals_canceled);
    }
  });

  it("journal every later application, applied or rejected, on the activity timeline", () => {
    const events = (ws.timeline[a.agent_id] ?? []).filter((e) => e.kind === "version");
    expect(events.map((e) => e.at).sort()).toEqual(a.versions.slice(1).map((v) => v.application.at).sort());
  });
});

describe("the oracle", () => {
  it("catches a fixture that calls a raised limit reducing", () => {
    expect(oracle({ path: "/risk/max_gross_exposure_usd", from: "1500", to: "2000", classification: "risk_reducing" })).toBe("risk_increasing");
    expect(oracle({ path: "/autonomy/approval/two_approver_above_usd", from: "400", to: null, classification: "risk_reducing" })).toBe("risk_increasing");
    expect(oracle({ path: "/capital/allocation_usd", from: "10000", to: null, classification: "risk_reducing" })).toBe("risk_increasing");
    expect(() => oracle({ path: "/behavior/description", from: "a", to: "b", classification: "neutral" })).toThrow(/no §9.2 row/);
  });
});
