// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { Agent, Mandate, Scenario, Workspace } from "@/fixtures/types";
import { AGENT_IDS, APPROVAL_IDS, NOW, SCENARIOS, buildWorkspace, findAgent } from "@/fixtures/workspace";
import { type VersionCase, oracle, overall, versionInvariants } from "@/test/version-invariants";
import { dec, mul, sub } from "./decimal";
import { addMs, mandateVersion } from "./fixture-journey";
import {
  EDIT_PATHS,
  type EditPath,
  type Edits,
  type FieldValue,
  type Origin,
  type Proposal,
  type Recorded,
  diffMandate,
  editableFields,
  fieldFor,
  inputText,
  propose,
  reachSafePoint,
  readInput,
  recordChange,
  validate,
} from "./mandate-change";

interface RefCase {
  id: string;
  kind: string;
  base: string;
  patch: Array<{ op: string; path: string; value: FieldValue }>;
  context?: { other_allocations_usd?: string; approver_users?: number };
  expect: { classification?: string; changed_paths?: string[]; old_version?: string; new_version?: string; step_up_required?: boolean; violations?: string[] };
}

const refcases = JSON.parse(readFileSync(new URL("../../../fixtures/refcases/mandate.json", import.meta.url), "utf8")) as {
  bases: Record<string, { mandate: Mandate }>;
  validation_context_defaults: { account_equity_usd: string; other_allocations_usd: string; approver_users: number };
  cases: RefCase[];
};

const editable = new Set<string>(EDIT_PATHS);
const byId = (id: string) => {
  const found = refcases.cases.find((c) => c.id === id);
  if (!found) throw new Error(`no reference case ${id}`);
  return found;
};
const editsFrom = (c: RefCase): Edits => Object.fromEntries(c.patch.map((p) => [p.path, p.value])) as Edits;

describe("the reference change cases on the fields an owner edits (mandate spec §9.2)", () => {
  const cases = refcases.cases.filter((c) => c.kind === "change" && c.patch.length > 0 && c.patch.every((p) => p.op === "replace" && editable.has(p.path)));

  it("are the seven this replays, so a new one fails here until it is replayed", () => {
    expect(cases.map((c) => c.id)).toEqual(["MC-C06", "MC-C09", "MC-C10", "MC-C26", "MC-C27", "MC-C28", "MC-C29"]);
  });

  it.each(cases.map((c) => [c.id, c] as const))("%s: reproduces the changed paths, classification, step-up and both hashes", (_id, c) => {
    const base = refcases.bases[c.base].mandate;
    const d = diffMandate(base, editsFrom(c));
    expect(mandateVersion(base)).toBe(c.expect.old_version);
    expect(d.version).toBe(c.expect.new_version);
    expect(d.changes.map((x) => x.path)).toEqual(c.expect.changed_paths);
    expect(d.classification).toBe(c.expect.classification);
    expect(d.stepUp).toBe(c.expect.step_up_required);
    expect(d.classification).toBe(overall(d.changes));
  });
});

describe("validation on the edited fields replays the reference semantic cases (§4.1)", () => {
  const defaults = refcases.validation_context_defaults;
  const ctx = (c: RefCase) => ({
    roomUsd: sub(dec(defaults.account_equity_usd), dec(c.context?.other_allocations_usd ?? defaults.other_allocations_usd)),
    approverUsers: c.context?.approver_users ?? defaults.approver_users,
  });

  it.each(["MC-V03", "MC-V20", "MC-V21", "MC-V39"])("%s: reports the case's violations among the rules these fields can break", (id) => {
    const c = byId(id);
    const m = diffMandate(refcases.bases[c.base].mandate, editsFrom(c)).mandate;
    const checked = new Set(["V-002", "V-013", "V-014", "V-016", "V-024"]);
    expect(validate(m, ctx(c)).map((r) => r.rule)).toEqual((c.expect.violations ?? []).filter((v) => checked.has(v)));
  });

  it("passes every reference base it is not meant to fail", () => {
    for (const name of ["btc_accumulator", "two_stock_swing"]) {
      expect(validate(refcases.bases[name].mandate, { roomUsd: dec(defaults.account_equity_usd), approverUsers: defaults.approver_users })).toEqual([]);
    }
  });

  it("holds the retail profile's lifetime-loss ceiling and approval-window minimum (§4.3)", () => {
    const base = refcases.bases.btc_accumulator.mandate;
    const room = { roomUsd: dec("25000"), approverUsers: 2 };
    expect(validate(diffMandate(base, { "/capital/max_loss_from_allocation": "0.2" }).mandate, room)).toEqual([]);
    expect(validate(diffMandate(base, { "/capital/max_loss_from_allocation": "0.21" }).mandate, room).map((r) => r.rule)).toEqual(["§4.3"]);
    expect(validate(diffMandate(base, { "/autonomy/approval/timeout_s": 60 }).mandate, room).map((r) => r.rule)).toEqual(["§4.3"]);
    expect(validate(diffMandate(base, { "/notifications/quiet_hours/end": "23:00" }).mandate, room).map((r) => r.rule)).toEqual(["V-016"]);
  });
});

describe("the owner's values", () => {
  const field = (path: EditPath) => fieldFor(path);

  it("read dollars, percentages, counts, minutes and times into the document's units", () => {
    expect(readInput(field("/risk/max_order_usd"), "$1,200.50")).toEqual({ ok: true, value: "1200.5" });
    expect(readInput(field("/risk/max_daily_loss"), "1.5%")).toEqual({ ok: true, value: "0.015" });
    expect(readInput(field("/risk/max_orders_per_day"), "60")).toEqual({ ok: true, value: 60 });
    expect(readInput(field("/autonomy/approval/timeout_s"), "15")).toEqual({ ok: true, value: 900 });
    expect(readInput(field("/notifications/quiet_hours/start"), "7:30")).toEqual({ ok: true, value: "07:30" });
    expect(readInput(field("/autonomy/approval/two_approver_above_usd"), "")).toEqual({ ok: true, value: null });
  });

  it("refuse what is not a value rather than guess", () => {
    for (const [path, raw] of [
      ["/risk/max_order_usd", "eight hundred"],
      ["/risk/max_order_usd", "0"],
      ["/risk/max_order_usd", "12.345"],
      ["/risk/max_daily_loss", "-1"],
      ["/risk/max_orders_per_day", "2.5"],
      ["/notifications/quiet_hours/start", "25:00"],
      ["/capital/allocation_usd", ""],
    ] as const) {
      expect(readInput(field(path), raw).ok, `${path} ${raw}`).toBe(false);
    }
  });

  it("round-trip every editable value of every fixture mandate", () => {
    for (const agent of buildWorkspace("normal").agents) {
      for (const f of editableFields(agent.mandate)) {
        const value = valueOf(agent.mandate, f.path);
        expect(readInput(f, inputText(f, value)), `${agent.label} ${f.path}`).toEqual({ ok: true, value });
      }
    }
  });

  it("treat a value written another way as no change, so the document and its hash stay as they were", () => {
    const btc = buildWorkspace("normal").agents[0];
    const longer = (v: string) => (v.includes(".") ? `${v}0` : `${v}.0`);
    const diff = diffMandate(btc.mandate, {
      "/risk/max_order_usd": longer(btc.mandate.risk.max_order_usd),
      "/risk/max_position_fraction": longer(btc.mandate.risk.max_position_fraction),
    });
    expect(diff.changes).toEqual([]);
    expect(diff.mandate).toEqual(btc.mandate);
  });
});

function valueOf(m: Mandate, path: EditPath): FieldValue {
  return path
    .split("/")
    .slice(1)
    .reduce<unknown>((node, key) => (node as Record<string, unknown>)[key], m) as FieldValue;
}

const FORM: Origin = { kind: "form" };
const later = (minutes: number) => addMs(NOW, minutes * 60_000);

function agentIn(ws: Workspace, id: string): Agent {
  const a = findAgent(ws, id);
  if (!a) throw new Error(`no agent ${id}`);
  return a;
}

describe("applying a version (§2.2)", () => {
  it("applies a risk-reducing version when confirmed, cancels the request waiting, and records the owner's values as entered", () => {
    const ws = buildWorkspace("normal");
    const p = propose(ws, agentIn(ws, AGENT_IDS.swing), { "/risk/max_order_usd": "800" });
    expect(p).toMatchObject({ classification: "risk_reducing", stepUp: false, refusals: [], number: 3 });
    const { ws: next, recorded } = recordChange(ws, p, FORM, later(1));
    expect(recorded).toEqual({ result: "applied" });
    const swing = agentIn(next, AGENT_IDS.swing);
    expect(swing.mandate.risk.max_order_usd).toBe("800");
    expect(swing.mandate_version).toBe(p.version);
    expect(swing.versions.at(-1)?.application).toEqual({ result: "applied", at: later(1), approvals_canceled: 1 });
    expect(next.approvals.find((a) => a.approval_id === APPROVAL_IDS.swingXyz)).toMatchObject({ status: "superseded", resolution: { cancel_reason: "version_applied", at: later(1) } });
    expect(swing.provenance.find((x) => x.path === "/risk/max_order_usd")).toEqual({ path: "/risk/max_order_usd", provenance: "user_entered" });
    expect(next.timeline[AGENT_IDS.swing][0]).toMatchObject({ kind: "version", at: later(1), text: "Mandate version 3 applied: Largest order from $1,000.00 to $800.00." });
  });

  it("records a value from a message as stated, with the owner's words", () => {
    const ws = buildWorkspace("normal");
    const p = propose(ws, agentIn(ws, AGENT_IDS.lmn), { "/risk/max_daily_loss": "0.01" });
    const { ws: next } = recordChange(ws, p, { kind: "message", quote: "lower the daily loss limit to 1%" }, later(1));
    expect(agentIn(next, AGENT_IDS.lmn).provenance.find((x) => x.path === "/risk/max_daily_loss")).toEqual({
      path: "/risk/max_daily_loss",
      provenance: "user_stated",
      quote: "lower the daily loss limit to 1%",
    });
  });

  it("cancels a working opening order that breaks the new limits, and leaves protection alone", () => {
    const ws = buildWorkspace("normal");
    const btc = agentIn(ws, AGENT_IDS.btc);
    const buy = btc.orders.find((o) => o.purpose === "increase");
    expect(mul(dec(buy?.qty ?? "0"), dec(buy?.limit_price ?? "0"))).toBe(dec("559"));
    const { ws: next } = recordChange(ws, propose(ws, btc, { "/risk/max_order_usd": "500" }), FORM, later(1));
    const after = agentIn(next, AGENT_IDS.btc);
    expect(after.orders.map((o) => o.purpose)).toEqual(["protective"]);
    expect(after.past_orders[0]).toMatchObject({ client_order_id: buy?.client_order_id, state: "Canceled", closed_at: later(1) });
    expect(next.timeline[AGENT_IDS.btc].map((e) => e.kind).slice(0, 2)).toEqual(["order", "version"]);
  });

  it("holds a risk-increasing version until a safe point, then applies it", () => {
    const ws = buildWorkspace("normal");
    const p = propose(ws, agentIn(ws, AGENT_IDS.btc), { "/risk/max_order_usd": "1200" });
    expect(p).toMatchObject({ classification: "risk_increasing", stepUp: true, refusals: [] });
    const held = recordChange(ws, p, FORM, later(1));
    expect(held.recorded).toEqual({ result: "pending" });
    expect(agentIn(held.ws, AGENT_IDS.btc).mandate.risk.max_order_usd).toBe("1000");
    const applied = reachSafePoint(held.ws, AGENT_IDS.btc, p.version, FORM, later(2));
    expect(applied?.recorded).toEqual({ result: "applied" });
    expect(agentIn(applied?.ws ?? ws, AGENT_IDS.btc)).toMatchObject({ mandate_version: p.version, mandate: { risk: { max_order_usd: "1200" } } });
  });

  it("keeps a risk-increasing version waiting while an order is in an unknown state", () => {
    const ws = buildWorkspace("unknown-order");
    const p = propose(ws, agentIn(ws, AGENT_IDS.swing), { "/risk/max_orders_per_day": 60 });
    const held = recordChange(ws, p, FORM, later(1));
    expect(held.recorded).toEqual({ result: "pending" });
    expect(reachSafePoint(held.ws, AGENT_IDS.swing, p.version, FORM, later(2))).toBeNull();
  });

  it("rejects a waiting version once another applies first, since it was diffed against a version no longer in effect", () => {
    const ws = buildWorkspace("unknown-order");
    const raise = propose(ws, agentIn(ws, AGENT_IDS.swing), { "/risk/max_orders_per_day": 60 });
    const held = recordChange(ws, raise, FORM, later(1)).ws;
    const lower = propose(held, agentIn(held, AGENT_IDS.swing), { "/risk/max_order_usd": "800" });
    expect(lower.number).toBe(4);
    const { ws: next, recorded } = recordChange(held, lower, FORM, later(2));
    expect(recorded).toEqual({ result: "applied" });
    const versions = agentIn(next, AGENT_IDS.swing).versions;
    expect(versions[2].application).toMatchObject({ result: "rejected", at: later(2) });
    expect(versions[3].application).toMatchObject({ result: "applied" });
  });

  it("refuses a change shown against a version that is no longer in effect, and makes no version of it", () => {
    const ws = buildWorkspace("normal");
    const stale = propose(ws, agentIn(ws, AGENT_IDS.lmn), { "/risk/max_order_usd": "900" });
    const moved = recordChange(ws, propose(ws, agentIn(ws, AGENT_IDS.lmn), { "/risk/max_order_usd": "800" }), FORM, later(1)).ws;
    const { ws: after, recorded } = recordChange(moved, stale, FORM, later(2));
    expect(recorded.result).toBe("rejected");
    expect(after).toBe(moved);
  });

  it("refuses an allocation increase while a limit is latched, before and at application (§5.1)", () => {
    const ws = buildWorkspace("drawdown");
    const btc = agentIn(ws, AGENT_IDS.btc);
    const p = propose(ws, btc, { "/capital/allocation_usd": "11000" });
    expect(p.refusals.map((r) => r.rule)).toEqual(["§5.1"]);
    const { ws: next, recorded } = recordChange(ws, p, FORM, later(1));
    expect(recorded.result).toBe("rejected");
    const after = agentIn(next, AGENT_IDS.btc);
    expect(after.mandate.capital.allocation_usd).toBe("10000");
    expect(after.versions.at(-1)?.application).toMatchObject({ result: "rejected", at: later(1) });
    expect(after.restrictions).toEqual(btc.restrictions);
  });

  it("never clears a latched limit, even with a version that lowers risk (MI-3)", () => {
    const ws = buildWorkspace("drawdown");
    const btc = agentIn(ws, AGENT_IDS.btc);
    const { ws: next, recorded } = recordChange(ws, propose(ws, btc, { "/risk/max_order_usd": "800" }), FORM, later(1));
    expect(recorded.result).toBe("applied");
    expect(agentIn(next, AGENT_IDS.btc)).toMatchObject({ mode: btc.mode, restrictions: btc.restrictions });
  });

  it("refuses an allocation below what the agent holds and has on order (§5.1)", () => {
    const ws = buildWorkspace("normal");
    const p = propose(ws, agentIn(ws, AGENT_IDS.btc), { "/capital/allocation_usd": "7000", "/risk/max_position_usd": "7000", "/risk/max_gross_exposure_usd": "7000" });
    expect(p.refusals.map((r) => r.rule)).toEqual(["§5.1"]);
  });

  it("scales the risk state by k = (E + Δ) ÷ E, rounding up, so no loss fraction falls (§5.1, MI-2)", () => {
    const ws = buildWorkspace("normal");
    const btc = agentIn(ws, AGENT_IDS.btc);
    const p = propose(ws, btc, { "/capital/allocation_usd": "8000", "/risk/max_position_usd": "8000", "/risk/max_gross_exposure_usd": "8000" });
    expect(p).toMatchObject({ classification: "risk_reducing", refusals: [] });
    const after = agentIn(recordChange(ws, p, FORM, later(1)).ws, AGENT_IDS.btc).state;
    const e = dec(btc.state.equity);
    const e1 = dec(after.equity);
    expect(e1).toBe(sub(e, dec("2000")));
    for (const key of ["high_water_mark", "equity_day_start", "capital_base"] as const) {
      const before = dec(btc.state[key]);
      const scaled = dec(after[key]);
      expect(scaled * e >= before * e1, key).toBe(true);
      expect(scaled * e - before * e1 < e, `${key} is rounded up by less than one place`).toBe(true);
    }
    const drawdown = (h: bigint, eq: bigint) => Number(((h - eq) * 10n ** 18n) / h);
    expect(drawdown(dec(after.high_water_mark), e1)).toBeGreaterThanOrEqual(drawdown(dec(btc.state.high_water_mark), e));
  });

  it("refuses every change to a stopped agent", () => {
    const ws = buildWorkspace("normal");
    const lmn = agentIn(ws, AGENT_IDS.lmn);
    lmn.mode = "stopped";
    expect(propose(ws, lmn, { "/risk/max_order_usd": "500" }).refusals.map((r) => r.rule)).toEqual(["stopped"]);
  });

  it("refuses a capital the account cannot cover once the other agents' allocations are counted (V-002)", () => {
    const ws = buildWorkspace("normal");
    const p = propose(ws, agentIn(ws, AGENT_IDS.lmn), { "/capital/allocation_usd": "9000" });
    expect(p.refusals.map((r) => r.rule)).toEqual(["V-002"]);
  });
});

/** The workspace's policy as a deployment could state it: on, off, not known, or not stated at all. */
type PolicyValue = boolean | null | "absent";

function withPolicy(ws: Workspace, value: PolicyValue, approverUsers = ws.approver_users): Workspace {
  const next: Workspace = { ...ws, approver_users: approverUsers, independent_approval_required: value === "absent" ? null : value };
  if (value === "absent") delete (next as Partial<Workspace>).independent_approval_required;
  return next;
}

const SECOND_PERSON =
  "This workspace needs a second person to approve a change that raises risk, and that approval can't be asked for here yet, so a passkey alone can't confirm it.";
const NO_SECOND_PERSON =
  "This workspace needs a second person to approve any change that doesn't lower risk, and no second person can approve in it, so only a change that lowers risk can be confirmed.";

describe("independent approval (mandate spec §4.3, V-047, DEC-444; interim until a second user can approve)", () => {
  const raise = { "/capital/max_loss_from_allocation": "0.15" } satisfies Edits;
  const lower = { "/risk/max_order_usd": "800" } satisfies Edits;
  const neutral = { "/notifications/quiet_hours/start": "22:00" } satisfies Edits;
  const swingIn = (ws: Workspace) => agentIn(ws, AGENT_IDS.swing);

  it.each([true, null, "absent"] as const)("refuses a risk-increasing version when the policy is %s, and records it as rejected, never waiting or applied", (value) => {
    const ws = withPolicy(buildWorkspace("normal"), value);
    const p = propose(ws, swingIn(ws), raise);
    expect(p.classification).toBe("risk_increasing");
    expect(p.refusals).toEqual([{ rule: "V-047", text: SECOND_PERSON }]);
    const { ws: next, recorded } = recordChange(ws, p, FORM, later(1));
    expect(recorded.result).toBe("rejected");
    const after = swingIn(next);
    expect(after.mandate.capital.max_loss_from_allocation).toBe("0.1");
    expect(after.mandate_version).toBe(swingIn(ws).mandate_version);
    expect(after.versions.at(-1)?.application).toMatchObject({ result: "rejected", at: later(1) });
    expect(after.versions.some((v) => v.application.result === "pending")).toBe(false);
  });

  it.each([true, null, "absent"] as const)("lets a risk-reducing or a neutral version apply as before when the policy is %s and two people can approve", (value) => {
    const ws = withPolicy(buildWorkspace("normal"), value);
    for (const edits of [lower, neutral]) {
      const p = propose(ws, swingIn(ws), edits);
      expect(p.refusals).toEqual([]);
      expect(recordChange(ws, p, FORM, later(1)).recorded).toEqual({ result: "applied" });
    }
  });

  it("changes nothing when the policy is off: a risk-increasing version takes the passkey and waits for its safe point", () => {
    const ws = withPolicy(buildWorkspace("normal"), false);
    const p = propose(ws, swingIn(ws), raise);
    expect(p).toMatchObject({ classification: "risk_increasing", stepUp: true, refusals: [] });
    expect(recordChange(ws, p, FORM, later(1)).recorded).toEqual({ result: "pending" });
  });

  it("is off in every fixture scenario, as the app behaved before it knew the policy", () => {
    for (const { id } of SCENARIOS) expect(buildWorkspace(id).independent_approval_required, id).toBe(false);
  });

  it("in a workspace with fewer than two people who can approve, lets only a risk-reducing version through (DEC-444)", () => {
    const ws = withPolicy(buildWorkspace("normal"), true, 1);
    expect(propose(ws, swingIn(ws), lower).refusals).toEqual([]);
    for (const edits of [raise, neutral]) {
      const p = propose(ws, swingIn(ws), edits);
      expect(p.refusals).toEqual([{ rule: "V-047", text: NO_SECOND_PERSON }]);
      expect(recordChange(ws, p, FORM, later(1)).recorded.result).toBe("rejected");
    }
  });

  it("rejects a waiting risk-increasing version at application once the policy requires independent approval", () => {
    const ws = withPolicy(buildWorkspace("normal"), false);
    const p = propose(ws, swingIn(ws), raise);
    const held = recordChange(ws, p, FORM, later(1));
    expect(held.recorded).toEqual({ result: "pending" });
    const applied = reachSafePoint(withPolicy(held.ws, true), AGENT_IDS.swing, p.version, FORM, later(2));
    expect(applied?.recorded).toEqual({ result: "rejected", reason: SECOND_PERSON });
    expect(swingIn(applied?.ws ?? ws).mandate.capital.max_loss_from_allocation).toBe("0.1");
  });
});

/** mulberry32: a fixed sequence per seed, so a failure names its seed and replays. */
function random(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomValue(rand: () => number, m: Mandate, path: EditPath): FieldValue {
  const pick = <T,>(xs: readonly T[]) => xs[Math.floor(rand() * xs.length)];
  const now = valueOf(m, path);
  const factor = pick(["0.5", "0.8", "0.9", "1.1", "1.25", "1.5"]);
  switch (fieldFor(path).unit) {
    case "usd":
      if (path === "/autonomy/approval/two_approver_above_usd") return pick([null, "300", "500", "800"]);
      return String(Number(mul(dec(String(now)), dec(factor)) / 10n ** 10n) / 100);
    case "percent":
      return pick(["0.01", "0.02", "0.03", "0.05", "0.08", "0.1", "0.15", "0.2", "0.25"]);
    case "count":
      return pick([10, 30, 50, 60, 100]);
    case "minutes":
      return pick([300, 600, 900, 1200]);
    case "time":
      return pick(["21:00", "22:30", "23:00", "06:00", "07:00"]);
    default:
      throw new Error(`no generator for ${path}`);
  }
}

const SCENARIO_POOL: Scenario[] = ["normal", "approvals", "drawdown", "unknown-order", "paused"];

/**
 * Random sequences of edits, confirmations and safe points on the fixture workspaces. Each
 * proposal's classification is checked against the independent §9.2 oracle, and every workspace
 * left behind must keep the version invariants the recorded scenarios keep.
 */
interface Step {
  label: string;
  proposal: Proposal;
  outcome: Recorded["result"];
}

function sequence(seed: number): { steps: Step[]; cases: VersionCase[] } {
  const rand = random(seed);
  let ws = buildWorkspace(SCENARIO_POOL[seed % SCENARIO_POOL.length]);
  let minute = 1;
  const steps: Step[] = [];
  for (let step = 0; step < 6; step++) {
    const agent = ws.agents[Math.floor(rand() * ws.agents.length)];
    const fields = editableFields(agent.mandate);
    const edits: Edits = {};
    for (let i = 0, n = 1 + Math.floor(rand() * 3); i < n; i++) {
      const f = fields[Math.floor(rand() * fields.length)];
      edits[f.path] = randomValue(rand, agent.mandate, f.path);
    }
    const p = propose(ws, agent, edits);
    if (p.changes.length === 0) continue;
    const { ws: recorded, recorded: outcome } = recordChange(ws, p, FORM, later(minute++));
    steps.push({ label: `seed ${seed} step ${step}`, proposal: p, outcome: outcome.result });
    ws = recorded;
    if (outcome.result === "pending" && rand() < 0.7) ws = reachSafePoint(ws, agent.agent_id, p.version, FORM, later(minute++))?.ws ?? ws;
  }
  return { steps, cases: ws.agents.map((a) => [`seed ${seed}`, a.label, a, ws] as const) };
}

const runs = Array.from({ length: 30 }, (_, seed) => sequence(seed));

describe("random change sequences", () => {
  const steps = runs.flatMap((r) => r.steps);

  it("cover every outcome, so the invariants below see applied, waiting and rejected versions", () => {
    expect(new Set(steps.map((s) => s.outcome))).toEqual(new Set(["applied", "pending", "rejected"]));
  });

  it("classify every path by the independent §9.2 oracle, take a passkey exactly when risk goes up, and hash the document they show", () => {
    for (const { label, proposal: p } of steps) {
      for (const c of p.changes) expect(c.classification, `${label} ${c.path}`).toBe(oracle(c));
      expect(p.classification, label).toBe(overall(p.changes));
      expect(p.stepUp, label).toBe(p.classification === "risk_increasing");
      expect(p.version, label).toBe(mandateVersion(p.mandate));
    }
  });

  it("apply only what passed every check, and reject whatever did not", () => {
    for (const { label, proposal, outcome } of steps) {
      if (outcome === "applied" || outcome === "pending") expect(proposal.refusals, label).toEqual([]);
      if (proposal.refusals.length > 0) expect(outcome, label).toBe("rejected");
    }
  });
});

versionInvariants(runs.flatMap((r) => r.cases));

const POLICY_POOL: readonly PolicyValue[] = [true, false, null, "absent"];

/**
 * Random edits under a random policy and approver count. The expected outcome is read off the
 * independent §9.2 oracle's class and the stated policy alone: only a stated `false` turns the
 * policy off, and with it on a version passes only if it lowers risk, or is neutral where two
 * people can approve.
 */
describe("random changes under independent approval", () => {
  const outcomes = Array.from({ length: 400 }, (_, i) => {
    const seed = 1000 + i;
    const rand = random(seed);
    const value = POLICY_POOL[i % POLICY_POOL.length];
    const approvers = rand() < 0.5 ? 1 : 2;
    const ws = withPolicy(buildWorkspace("normal"), value, approvers);
    const agent = ws.agents[Math.floor(rand() * ws.agents.length)];
    const fields = editableFields(agent.mandate);
    const edits: Edits = {};
    for (let n = 1 + Math.floor(rand() * 2); n > 0; n--) {
      const f = fields[Math.floor(rand() * fields.length)];
      edits[f.path] = randomValue(rand, agent.mandate, f.path);
    }
    const p = propose(ws, agent, edits);
    return { label: `seed ${seed}, policy ${String(value)}, ${approvers} approvers`, value, approvers, p, recorded: recordChange(ws, p, FORM, later(1)).recorded };
  }).filter((o) => o.p.changes.length > 0);

  it("see every class under every policy value", () => {
    for (const value of POLICY_POOL) {
      expect(new Set(outcomes.filter((o) => o.value === value).map((o) => overall(o.p.changes))), String(value)).toEqual(new Set(["risk_increasing", "risk_reducing", "neutral"]));
    }
  });

  it("never let a version the policy holds back wait or apply", () => {
    for (const { label, value, approvers, p, recorded } of outcomes) {
      const cls = overall(p.changes);
      const held = value !== false && (cls === "risk_increasing" || (cls === "neutral" && approvers < 2));
      expect(p.refusals.some((r) => r.rule === "V-047"), label).toBe(held);
      if (held) expect(recorded.result, label).toBe("rejected");
    }
  });
});
