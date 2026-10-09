import { describe, expect, it } from "vitest";
import type { Agent, ChangeClass, MandateChange, MandateVersionRecord, Workspace } from "@/fixtures/types";
import { dec } from "@/lib/decimal";

type Value = MandateChange["from"];

/** Mandate spec §9.2 "Maximums": allocation, `max_loss_from_allocation`, `max_*` and `stop_distance`. Larger is increasing, and null means unbounded. */
const MAXIMUMS = /^\/(capital\/(allocation_usd|max_loss_from_allocation)|risk\/max_[a-z_]+|protection\/stop_distance)$/;

const larger = (from: Value, to: Value) => (to === null ? from !== null : from !== null && dec(String(to)) > dec(String(from)));

/**
 * Only the §9.2 rows the fixtures and the edit fields use, read from the spec's table rather than
 * from the fixture or the code. A path with no row here fails, so a new change brings its rule with it.
 */
export function oracle({ path, from, to }: Pick<MandateChange, "path" | "from" | "to">): ChangeClass {
  if (path.startsWith("/notifications/quiet_hours/")) return "neutral";
  if (path === "/autonomy/approval/two_approver_above_usd") return to !== null && (from === null || dec(String(to)) < dec(String(from))) ? "risk_reducing" : "risk_increasing";
  if (path === "/autonomy/approval/timeout_s") return "risk_increasing";
  if (MAXIMUMS.test(path)) return larger(from, to) ? "risk_increasing" : "risk_reducing";
  throw new Error(`no §9.2 row in this oracle for ${path}`);
}

export function overall(changes: Array<Pick<MandateChange, "path" | "from" | "to">>): ChangeClass {
  const classes = changes.map(oracle);
  if (classes.includes("risk_increasing")) return "risk_increasing";
  if (classes.includes("risk_reducing")) return "risk_reducing";
  return "neutral";
}

export function at(doc: unknown, pointer: string): unknown {
  return pointer
    .split("/")
    .slice(1)
    .reduce<unknown>((node, key) => (node as Record<string, unknown> | null)?.[key], doc);
}

function inEffect(a: Agent, iso: string): MandateVersionRecord | undefined {
  return a.versions.filter((v) => v.application.result === "applied" && Date.parse(v.application.at) <= Date.parse(iso)).at(-1);
}

const settledAt = (v: MandateVersionRecord): string | null => (v.application.result === "pending" ? null : v.application.at);

export type VersionCase = readonly [string, string, Agent, Workspace];

/** The invariants every agent's versions keep, in the recorded scenarios and in any workspace a change leaves behind. */
export function versionInvariants(cases: readonly VersionCase[]) {
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
        const settled = settledAt(v);
        if (v.classification === "risk_increasing") {
          if (settled !== null && v.application.result === "applied") expect(Date.parse(settled) - Date.parse(v.confirmed_at)).toBeGreaterThan(0);
        } else {
          expect(v.application.result).not.toBe("pending");
          if (v.application.result === "applied") expect(v.application.at).toBe(v.confirmed_at);
        }
      }
      expect(a.versions.filter((v) => v.application.result === "pending").length).toBeLessThanOrEqual(1);
    });

    it("diff each version against the one in effect, and end at the agent's mandate and version", () => {
      const doc = firstDocument(a);
      let current = a.versions[0].mandate_version;
      for (const v of a.versions.slice(1)) {
        expect(v.previous).toBe(current);
        for (const c of v.changes) expect(c.from, c.path).toEqual(at(doc, c.path));
        if (v.application.result !== "applied") continue;
        for (const c of v.changes) set(doc, c.path, c.to);
        current = v.mandate_version;
      }
      expect(current).toBe(a.mandate_version);
      expect(doc).toEqual(a.mandate);
    });

    it("bind every approval to the version in effect when it was requested, and count the ones each version canceled", () => {
      const mine = ws.approvals.filter((x) => x.agent_id === a.agent_id);
      for (const approval of mine) expect(approval.bound.mandate_version, approval.approval_id).toBe(inEffect(a, approval.requested_at)?.mandate_version);
      for (const v of a.versions.slice(1)) {
        if (v.application.result !== "applied") continue;
        const appliedAt = v.application.at;
        const canceled = mine.filter((x) => x.resolution?.cancel_reason === "version_applied" && x.resolution.at === appliedAt);
        expect(canceled).toHaveLength(v.application.approvals_canceled);
      }
    });

    it("bind every gate decision to the version in force when the gate decided, as GateDecided's config_refs do", () => {
      for (const d of ws.decisions.filter((x) => x.agent_id === a.agent_id)) expect(d.mandate_version, d.event_id).toBe(inEffect(a, d.at)?.mandate_version);
    });

    it("journal every later application, applied or rejected, on the activity timeline", () => {
      const events = (ws.timeline[a.agent_id] ?? []).filter((e) => e.kind === "version" && !e.text.startsWith("Mandate version 1 confirmed"));
      expect(events.map((e) => e.at).sort()).toEqual(
        a.versions
          .slice(1)
          .flatMap((v) => settledAt(v) ?? [])
          .sort(),
      );
    });
  });
}

function set(doc: unknown, pointer: string, value: Value) {
  const keys = pointer.split("/").slice(1);
  const last = keys.pop();
  const parent = keys.reduce<Record<string, unknown>>((node, key) => node[key] as Record<string, unknown>, doc as Record<string, unknown>);
  if (last) parent[last] = value;
}

/** Version 1's document: the agent's mandate with every applied change undone, newest first. */
function firstDocument(a: Agent): unknown {
  const doc = structuredClone(a.mandate);
  for (const v of a.versions.slice(1).reverse()) {
    if (v.application.result === "applied") for (const c of v.changes) set(doc, c.path, c.from);
  }
  return doc;
}
