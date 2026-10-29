import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newAgentRequest } from "@/test/new-agent";
import { Providers } from "@/components/providers";
import { lastPrice } from "@/fixtures/market";
import type { InstrumentRef, Mandate, Workspace } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { confirmStepUpDigest } from "@/api/confirm";
import { BACKTEST_IDS } from "@/fixtures/backtests";
import { VERSION } from "@/fixtures/versions";
import { RECORD_AFTER_MS } from "@/test/harness";
import { RuntimeProbe, probed } from "@/test/runtime-probe";
import { CONFIRM_RECORD, DEPLOY_RECORD, setUpThrough } from "@/test/set-up";
import type { SetUp } from "./mock-runtime";
import { dec, mul, toDecimalString } from "./decimal";
import { INSTRUMENTS } from "@/fixtures/mandates";
import { fixtureAssetId } from "./fixture-ids";
import {
  type NewAgent,
  addMs,
  agentIdFor,
  canonicalJson,
  checkDeploy,
  claimedBy,
  deployAgent,
  fillApproved,
  firstProposal,
  mandateVersion,
  noteSkip,
  submitApproved,
  unallocatedUsd,
} from "./fixture-journey";

/** The fixtures' own instruments by symbol; any other symbol gets its fixture asset id, as the old set-up gave it. */
const KNOWN: Record<string, InstrumentRef> = Object.fromEntries(Object.values(INSTRUMENTS).map((i) => [i.symbol, i]));

/**
 * The fixture request with its money and instruments changed: a position and an order sized as the
 * old set-up sized them, a quarter and a tenth of the money, and the whole of it in total holdings.
 * `loss` is the owner's answer the old set-up read; the request keeps a tenth of the money.
 */
function newAgent(ws: Workspace, { money = "$3,000", symbols = ["MSFT"] }: { money?: string; loss?: string; symbols?: string[] } = {}): NewAgent {
  const request = newAgentRequest(ws.connection.connection_id);
  const usd = dec(money.replace(/[$,]/g, ""));
  const mandate = structuredClone(request.mandate);
  mandate.name = `${symbols.join("-").toLowerCase()}-agent`;
  mandate.capital.allocation_usd = toDecimalString(usd);
  mandate.risk.max_gross_exposure_usd = toDecimalString(usd);
  mandate.risk.max_position_usd = toDecimalString(mul(usd, dec("0.25")));
  mandate.risk.max_order_usd = toDecimalString(mul(usd, dec("0.1")));
  mandate.universe.pinned_instruments = symbols.map((s) => KNOWN[s] ?? { asset_id: fixtureAssetId(s), symbol: s, asset_class: "us_equity" });
  mandate.universe.max_instruments = symbols.length;
  return { ...request, mandate };
}

/** Whole cents from a decimal string, computed apart from `lib/decimal`, so the checks below are an independent oracle. */
function cents(value: string): bigint {
  const [whole, frac = ""] = value.split(".");
  return BigInt(whole) * 100n + BigInt((frac + "00").slice(0, 2));
}

/** Approves the agent's first request the way the runtime does: the owner's answer, then the gate's re-run. */
function approve(ws: Workspace, approvalId: string, at: string): Workspace {
  const next = structuredClone(ws);
  const a = next.approvals.find((x) => x.approval_id === approvalId)!;
  a.approvals_so_far = [{ user_label: "You", at }];
  a.status = "acted";
  a.resolution = { at, text: "Approved." };
  return next;
}

function journeyTo(stage: "deployed" | "asked" | "submitted" | "filled") {
  const ws = buildWorkspace("normal");
  const request = newAgent(ws);
  const at = ws.now;
  const { ws: deployed, agentId } = deployAgent(ws, request, at, "test:1");
  if (stage === "deployed") return { ws: deployed, agentId, approvalId: "", request };
  const asked = firstProposal(deployed, agentId, addMs(at, 3200), "test:1");
  const approvalId = asked.approvals[0].approval_id;
  if (stage === "asked") return { ws: asked, agentId, approvalId, request };
  const submitted = submitApproved(approve(asked, approvalId, addMs(at, 5000)), approvalId, addMs(at, 6000));
  if (stage === "submitted") return { ws: submitted, agentId, approvalId, request };
  return { ws: fillApproved(submitted, approvalId, addMs(at, 7600)), agentId, approvalId, request };
}

describe("versions and IDs", () => {
  it("hashes a mandate by its content, not its key order", () => {
    const m = newAgent(buildWorkspace("normal")).mandate;
    const reordered = Object.fromEntries(Object.entries(m).reverse()) as unknown as Mandate;
    expect(canonicalJson(reordered)).toBe(canonicalJson(m));
    expect(mandateVersion(reordered)).toBe(mandateVersion(m));
    expect(mandateVersion(m)).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(mandateVersion({ ...m, capital: { ...m.capital, allocation_usd: "3001" } })).not.toBe(mandateVersion(m));
  });

  it("names the same agent for the same mandate and seed, and another for another seed", () => {
    const m = newAgent(buildWorkspace("normal")).mandate;
    expect(agentIdFor(m, "a")).toBe(agentIdFor(m, "a"));
    expect(agentIdFor(m, "a")).not.toBe(agentIdFor(m, "b"));
    expect(agentIdFor(m, "a")).toMatch(/^agt_[0-9A-HJKMNP-TV-Z]{26}$/);
  });

  it("moves an ISO time by milliseconds in its own offset", () => {
    expect(addMs("2026-09-28T14:05:20-04:00", 300_000)).toBe("2026-09-28T14:10:20-04:00");
    expect(addMs("2026-09-28T23:59:59-04:00", 1000)).toBe("2026-09-29T00:00:00-04:00");
  });
});

describe("the checks repeated when a deployment applies (V-002, V-006)", () => {
  it("allows exactly the unallocated equity and refuses a cent more", () => {
    const ws = buildWorkspace("normal");
    const room = unallocatedUsd(ws);
    expect(room).toBe(dec("3478.36"));
    expect(checkDeploy(ws, newAgent(ws, { money: "$3,478.36", loss: "$300" }).mandate)).toEqual({ ok: true });
    expect(checkDeploy(ws, newAgent(ws, { money: "$3,478.37", loss: "$300" }).mandate)).toEqual({
      ok: false,
      reason: "Your paper account has $3,478.36 that no agent uses, less than the $3,478.37 this agent asks for.",
    });
  });

  it("frees a stopped agent's allocation and instruments", () => {
    const ws = buildWorkspace("normal");
    expect(claimedBy(ws, "LMN")?.agent_id).toBe(AGENT_IDS.lmn);
    expect(checkDeploy(ws, newAgent(ws, { symbols: ["LMN"] }).mandate)).toEqual({ ok: false, reason: "LMN is already traded by Agent 3. One agent trades an instrument on an account." });
    const stopped = structuredClone(ws);
    stopped.agents.find((a) => a.agent_id === AGENT_IDS.lmn)!.mode = "stopped";
    expect(unallocatedUsd(stopped)).toBe(dec("8478.36"));
    expect(claimedBy(stopped, "LMN")).toBeUndefined();
    expect(checkDeploy(stopped, newAgent(stopped, { money: "$5,000", loss: "$500", symbols: ["LMN"] }).mandate)).toEqual({ ok: true });
  });
});

/** The workspace's policy as a deployment could state it: on, off, not known, or not stated at all. */
type PolicyValue = boolean | null | "absent";

function withPolicy(value: PolicyValue, approverUsers?: number) {
  return (ws: Workspace): Workspace => {
    const next: Workspace = { ...ws, approver_users: approverUsers ?? ws.approver_users, independent_approval_required: value === "absent" ? null : value };
    if (value === "absent") delete (next as Partial<Workspace>).independent_approval_required;
    return next;
  };
}

const DEPLOY_SECOND_PERSON = "This workspace needs a second person to approve a new agent, and that approval can't be asked for here yet, so a passkey alone can't create it.";

describe("independent approval at deployment (mandate spec §4.3, V-047, DEC-444; interim until a second user can approve)", () => {
  it.each([true, null, "absent"] as const)("refuses every deployment when the policy is %s, whatever the approver count, and names V-047 only as the rule", (value) => {
    for (const approvers of [1, 2, 5]) {
      const ws = withPolicy(value, approvers)(buildWorkspace("normal"));
      const check = checkDeploy(ws, newAgent(ws).mandate);
      expect(check, `${approvers} approvers`).toEqual({ ok: false, rule: "V-047", reason: DEPLOY_SECOND_PERSON });
      expect(check.ok ? "" : check.reason).not.toContain("V-047");
    }
  });

  it("refuses under the policy even a deployment V-002 or V-006 would also refuse, with V-047's reason", () => {
    const ws = withPolicy(true)(buildWorkspace("normal"));
    expect(checkDeploy(ws, newAgent(ws, { money: "$3,478.37", loss: "$300" }).mandate)).toEqual({ ok: false, rule: "V-047", reason: DEPLOY_SECOND_PERSON });
    expect(checkDeploy(ws, newAgent(ws, { symbols: ["LMN"] }).mandate)).toEqual({ ok: false, rule: "V-047", reason: DEPLOY_SECOND_PERSON });
  });

  it("checks exactly as before when the policy is off, with one approver or two", () => {
    for (const approvers of [1, 2]) {
      const ws = withPolicy(false, approvers)(buildWorkspace("normal"));
      expect(checkDeploy(ws, newAgent(ws).mandate)).toEqual({ ok: true });
      expect(checkDeploy(ws, newAgent(ws, { symbols: ["LMN"] }).mandate)).toEqual({ ok: false, reason: "LMN is already traded by Agent 3. One agent trades an instrument on an account." });
    }
  });
});

describe("deploying", () => {
  it("adds a running agent holding nothing, at version 1 of the confirmed mandate, without touching the input", () => {
    const ws = buildWorkspace("normal");
    const before = structuredClone(ws);
    const request = newAgent(ws);
    const { ws: next, agentId } = deployAgent(ws, request, ws.now, "test:1");
    expect(ws).toEqual(before);
    const agent = next.agents.find((a) => a.agent_id === agentId)!;
    expect(next.agents).toHaveLength(ws.agents.length + 1);
    expect(agent).toMatchObject({ label: "Agent 4", mode: "normal", startup: "ready", positions: [], orders: [], restrictions: [], deployed_at: ws.now });
    expect(agent.mandate_version).toBe(mandateVersion(request.mandate));
    expect(agent.versions).toEqual([
      { mandate_version: agent.mandate_version, previous: null, confirmed_at: ws.now, step_up: true, classification: null, changes: [], application: { result: "applied", at: ws.now, approvals_canceled: 0 } },
    ]);
    expect(agent.state).toMatchObject({ equity: "3000", capital_base: "3000", high_water_mark: "3000", orders_today: 0 });
    expect(next.timeline[agentId].map((e) => e.text)).toEqual(["Mandate version 1 confirmed by you and deployed to paper, with $3,000.00 of simulated money."]);
    expect(next.connection.account_equity).toBe(ws.connection.account_equity);
  });
});

describe("the first check", () => {
  it("asks before one limit buy at the fixture price, sized inside the order and position limits", () => {
    const { ws, agentId, request } = journeyTo("asked");
    const approval = ws.approvals[0];
    expect(approval).toMatchObject({ agent_id: agentId, status: "delivered", approvers_required: 1, approvals_so_far: [] });
    expect(approval.bound).toMatchObject({ symbol: "MSFT", side: "buy", purpose: "open", limit: lastPrice("MSFT"), decided_by: "default:ask", mandate_version: mandateVersion(request.mandate) });
    const budget = [request.mandate.risk.max_order_usd, request.mandate.risk.max_position_usd].map(cents).reduce((a, b) => (a < b ? a : b));
    const each = cents(approval.bound.limit);
    expect(BigInt(approval.bound.qty)).toBe(budget / each);
    expect(BigInt(approval.bound.qty) * each <= budget).toBe(true);
    expect((BigInt(approval.bound.qty) + 1n) * each > budget).toBe(true);
    expect(Date.parse(approval.deadline) - Date.parse(approval.requested_at)).toBe(request.mandate.autonomy.approval.timeout_s * 1000);
    expect(ws.decisions[0]).toMatchObject({ agent_id: agentId, verdict: "allow", approval_id: approval.approval_id });
    expect(ws.decisions[0].client_order_id).toBeUndefined();
    expect(ws.agents.find((a) => a.agent_id === agentId)!.orders).toEqual([]);
  });

  it("sizes every affordable buy at or under both limits, for any allocation", () => {
    for (const money of ["$250", "$1,000", "$1,999.99", "$3,000", "$3,478.36"]) {
      const ws = buildWorkspace("normal");
      const request = newAgent(ws, { money, loss: "10%", symbols: ["AAPL", "MSFT", "NVDA"] });
      const { ws: deployed, agentId } = deployAgent(ws, request, ws.now, money);
      const asked = firstProposal(deployed, agentId, ws.now, money);
      const approval = asked.approvals.find((a) => a.agent_id === agentId);
      const cap = [request.mandate.risk.max_order_usd, request.mandate.risk.max_position_usd].map(cents).reduce((a, b) => (a < b ? a : b));
      if (!approval) {
        for (const s of ["AAPL", "MSFT", "NVDA"]) expect(cents(lastPrice(s)) > cap, `${money} ${s}`).toBe(true);
        expect(asked.timeline[agentId][0].text).toMatch(/^No buy proposed: one share of each symbol costs more than/);
        continue;
      }
      expect(BigInt(approval.bound.qty) >= 1n, money).toBe(true);
      expect(BigInt(approval.bound.qty) * cents(approval.bound.limit) <= cap, money).toBe(true);
    }
  });

  it("proposes nothing for an agent that is not running normally", () => {
    const { ws, agentId } = journeyTo("deployed");
    const paused = structuredClone(ws);
    paused.agents.find((a) => a.agent_id === agentId)!.mode = "paused";
    expect(firstProposal(paused, agentId, ws.now, "x")).toBe(paused);
  });

  it("proposes nothing when the score is below the entry threshold", () => {
    const { ws, agentId } = journeyTo("deployed");
    const strict = structuredClone(ws);
    strict.agents.find((a) => a.agent_id === agentId)!.mandate.behavior.sizing.entry_threshold = "0.9";
    const next = firstProposal(strict, agentId, ws.now, "x");
    expect(next.approvals).toEqual(strict.approvals);
    expect(next.timeline[agentId][0].text).toMatch(/^No buy proposed: MSFT scored 0\.\d+, below the entry threshold of 0\.9\.$/);
  });
});

describe("after the owner answers", () => {
  it("sends nothing for an approval the gate has not re-run", () => {
    const { ws, approvalId } = journeyTo("asked");
    expect(submitApproved(ws, approvalId, ws.now)).toBe(ws);
    expect(fillApproved(ws, approvalId, ws.now)).toBe(ws);
  });

  it("submits an approved buy once, as a resting day limit order", () => {
    const { ws, agentId, approvalId } = journeyTo("submitted");
    const agent = ws.agents.find((a) => a.agent_id === agentId)!;
    expect(agent.orders).toHaveLength(1);
    expect(agent.orders[0]).toMatchObject({ side: "buy", qty: "6", filled_qty: "0", limit_price: lastPrice("MSFT"), purpose: "open", state: "Accepted", time_in_force: "day" });
    expect(agent.state.orders_today).toBe(1);
    expect(ws.decisions[0]).toMatchObject({ approval_id: approvalId, client_order_id: agent.orders[0].client_order_id });
    expect(submitApproved(ws, approvalId, ws.now)).toBe(ws);
  });

  it("fills at or below the limit, places the protective stop for the quantity bought, and fills only once", () => {
    const { ws, agentId, approvalId, request } = journeyTo("filled");
    const agent = ws.agents.find((a) => a.agent_id === agentId)!;
    const [fill] = agent.fills;
    expect(agent.fills).toHaveLength(1);
    expect(cents(fill.price) <= cents(lastPrice("MSFT"))).toBe(true);
    expect(agent.past_orders[0]).toMatchObject({ state: "Filled", filled_qty: "6", client_order_id: fill.client_order_id });
    expect(agent.positions).toEqual([expect.objectContaining({ qty: "6", broker_qty: "6", avg_cost: lastPrice("MSFT") })]);
    const stop = agent.orders.find((o) => o.purpose === "protective")!;
    expect(stop).toMatchObject({ side: "sell", qty: "6", time_in_force: "gtc", state: "Accepted" });
    const expectedStop = (cents(fill.price) * (100n - cents(request.mandate.protection.stop_distance ?? "0"))) / 100n;
    expect(cents(stop.stop_price!)).toBe(expectedStop);
    expect(agent.positions[0].protection).toMatchObject({ kind: "bracket", stop_price: stop.stop_price });
    expect(agent.state.equity).toBe("3000.00");
    expect(ws.timeline[agentId].slice(0, 2).map((e) => e.kind)).toEqual(["protection", "fill"]);
    expect(fillApproved(ws, approvalId, ws.now)).toBe(ws);
  });

  it("notes a skip and sends nothing", () => {
    const { ws, agentId, approvalId } = journeyTo("asked");
    const skipped = structuredClone(ws);
    skipped.approvals[0].status = "rejected";
    const next = noteSkip(skipped, approvalId, ws.now);
    expect(next.timeline[agentId][0].text).toBe("You skipped buy 6 MSFT.");
    expect(next.agents.find((a) => a.agent_id === agentId)!.orders).toEqual([]);
    expect(noteSkip(ws, approvalId, ws.now)).toBe(ws);
  });
});

describe("the runtime's set-up: create the version, confirm it, deploy it (DEC-2401)", () => {
  function mount(scenario: Parameters<typeof buildWorkspace>[0] = "normal", workspace: (ws: Workspace) => Workspace = (ws) => ws) {
    render(
      <Providers workspace={workspace(buildWorkspace(scenario))} tick={false} recordAfterMs={RECORD_AFTER_MS}>
        <RuntimeProbe />
      </Providers>,
    );
  }

  const only = () => {
    const [setUp] = probed().setUps;
    return setUp;
  };
  const digest = (version: SetUp["version"]) => confirmStepUpDigest(version, null, null);

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("records each step on its own, in order, and changes nothing before the deployment is recorded", () => {
    mount();
    const request = newAgent(probed().ws);
    const before = structuredClone(probed().ws);
    let id = "";
    act(() => {
      id = probed().createVersion(request).id;
    });
    expect(only()).toMatchObject({ id, version: mandateVersion(request.mandate), created: { phase: "sent", base: null, classification: null, validated: false }, confirmed: null, deployed: null });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(only().created).toMatchObject({ phase: "recorded", recordedAt: before.now, base: null, classification: "risk_increasing", validated: true });
    expect(probed().ws).toEqual(before);

    act(() => probed().confirmVersion(id, CONFIRM_RECORD, digest(only().version)));
    expect(only().confirmed).toMatchObject({ phase: "sent", record: CONFIRM_RECORD, stepUp: digest(only().version) });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(only().confirmed?.phase).toBe("recorded");
    expect(only().deployed).toBeNull();
    expect(probed().ws).toEqual(before);

    act(() => probed().deployVersion(id, DEPLOY_RECORD));
    expect(only().deployed).toMatchObject({ phase: "sent", record: DEPLOY_RECORD });
    expect(probed().ws.agents).toHaveLength(before.agents.length);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const setUp = only();
    expect(setUp.deployed?.phase).toBe("recorded");
    expect(probed().ws.agents.at(-1)).toMatchObject({ agent_id: setUp.agentId, label: "Agent 4", mandate_version: setUp.version });
    expect(probed().ws.approvals.some((a) => a.agent_id === setUp.agentId)).toBe(false);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(probed().ws.approvals[0]).toMatchObject({ agent_id: setUp.agentId, status: "delivered" });
  });

  it("refuses a confirm of a version not recorded, and a deployment of a version not confirmed, recording nothing", () => {
    mount();
    let id = "";
    act(() => {
      id = probed().createVersion(newAgent(probed().ws)).id;
    });
    act(() => probed().confirmVersion(id, CONFIRM_RECORD, digest(only().version)));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(only().confirmed).toMatchObject({ phase: "undelivered", reason: "not_created" });
    act(() => probed().deployVersion(id, DEPLOY_RECORD));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(only().deployed).toMatchObject({ phase: "undelivered", reason: "not_confirmed" });
    expect(probed().ws.agents.some((a) => a.label === "Agent 4")).toBe(false);
  });

  it.each(["unreachable", "result-unknown"] as const)("refuses a confirm and a deployment whose step before is not recorded first, even in the %s scenario", (scenario) => {
    mount(scenario);
    let id = "";
    act(() => {
      id = probed().createVersion(newAgent(probed().ws)).id;
    });
    act(() => probed().confirmVersion(id, CONFIRM_RECORD, digest(only().version)));
    act(() => probed().deployVersion(id, DEPLOY_RECORD));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(only().confirmed).toMatchObject({ phase: "undelivered", reason: "not_created" });
    expect(only().deployed).toMatchObject({ phase: "undelivered", reason: "not_confirmed" });
  });

  it("refuses a risk-increasing confirm sent without a passkey check, as the API does (step_up_required)", () => {
    mount();
    let id = "";
    act(() => {
      id = probed().createVersion(newAgent(probed().ws)).id;
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    act(() => probed().confirmVersion(id, CONFIRM_RECORD, null));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(only().confirmed).toMatchObject({ phase: "undelivered", reason: "step_up_required" });
  });

  it("rejects at application a second deployment the first one used the room for, and deploys nothing", () => {
    mount();
    const first = newAgent(probed().ws, { money: "$2,000", loss: "$200", symbols: ["MSFT"] });
    const second = newAgent(probed().ws, { money: "$2,000", loss: "$200", symbols: ["AAPL"] });
    let ids: string[] = [];
    act(() => {
      ids = [probed().createVersion(first).id, probed().createVersion(second).id];
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(probed().setUps.map((s) => s.created.phase)).toEqual(["recorded", "recorded"]);
    act(() => {
      for (const s of probed().setUps) probed().confirmVersion(s.id, CONFIRM_RECORD, digest(s.version));
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    act(() => {
      for (const id of ids) probed().deployVersion(id, DEPLOY_RECORD);
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(probed().setUps.map((s) => s.deployed?.phase)).toEqual(["recorded", "rejected"]);
    expect(probed().setUps[1].deployed?.reason).toBe("Your paper account has $1,478.36 that no agent uses, less than the $2,000.00 this agent asks for.");
    expect(probed().setUps[1].confirmed?.phase).toBe("recorded");
    expect(probed().ws.agents.filter((a) => a.label === "Agent 4")).toHaveLength(1);
    expect(probed().ws.agents.some((a) => a.agent_id === probed().setUps[1].agentId)).toBe(false);
  });

  it("refuses at validation a version a deployment could not pass (V-002), so no version exists", () => {
    mount();
    act(() => {
      probed().createVersion(newAgent(probed().ws, { money: "$5,000", loss: "$500", symbols: ["MSFT"] }));
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(only().created).toMatchObject({ phase: "rejected", classification: null, validated: false, reason: "Your paper account has $3,478.36 that no agent uses, less than the $5,000.00 this agent asks for." });
  });

  it("finds the fixtures' completed backtest of a version, and none for a new one", () => {
    mount();
    expect(probed().backtestFor(VERSION.lmn)).toBe(BACKTEST_IDS.lmnCompleted);
    expect(probed().backtestFor(mandateVersion(newAgent(probed().ws).mandate))).toBeNull();
  });

  it("approves, submits, fills, and protects the first buy through the same timers as every response", () => {
    mount();
    setUpThrough(newAgent(probed().ws));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    const agentId = only().agentId;
    const approvalId = probed().ws.approvals[0].approval_id;
    act(() => probed().respond(approvalId, "approve", { screen: "D6", environment: "paper", shown: [], modelOutputExpanded: false }));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(probed().ws.agents.find((a) => a.agent_id === agentId)!.orders).toEqual([]);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 0.75));
    expect(probed().ws.agents.find((a) => a.agent_id === agentId)!.orders).toEqual([expect.objectContaining({ purpose: "open", state: "Accepted" })]);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const agent = probed().ws.agents.find((a) => a.agent_id === agentId)!;
    expect(agent.positions).toHaveLength(1);
    expect(agent.orders).toEqual([expect.objectContaining({ purpose: "protective", side: "sell" })]);
    expect(probed().ws.approvals[0].status).toBe("acted");
  });

  it.each([true, null, "absent"] as const)("refuses the version at validation while the policy is %s (V-047), so nothing is confirmed, deployed or asked", (value) => {
    mount("normal", withPolicy(value));
    const before = structuredClone(probed().ws);
    act(() => {
      probed().createVersion(newAgent(probed().ws));
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 4));
    const setUp = only();
    expect(setUp.created).toMatchObject({ phase: "rejected", rule: "V-047", reason: DEPLOY_SECOND_PERSON, validated: false });
    expect(setUp.confirmed).toBeNull();
    expect(setUp.deployed).toBeNull();
    expect(probed().ws.agents.map((a) => a.agent_id)).toEqual(before.agents.map((a) => a.agent_id));
    expect(probed().ws.approvals).toEqual(before.approvals);
    expect(probed().ws.timeline[setUp.agentId]).toBeUndefined();
  });

  it("rejects at application a deployment whose policy turned on after its confirmation (V-047), and deploys nothing", () => {
    mount("normal", withPolicy(false, 1));
    const before = structuredClone(probed().ws);
    const id = setUpThrough(newAgent(probed().ws));
    act(() => {
      // Deliberate direct mutation: the policy changes between the confirmation and the deployment applying.
      probed().ws.independent_approval_required = true;
      vi.advanceTimersByTime(RECORD_AFTER_MS);
    });
    const setUp = probed().setUps.find((s) => s.id === id)!;
    expect(setUp.confirmed?.phase).toBe("recorded");
    expect(setUp.deployed).toMatchObject({ phase: "rejected", rule: "V-047", reason: DEPLOY_SECOND_PERSON });
    expect(probed().ws.agents.map((a) => a.agent_id)).toEqual(before.agents.map((a) => a.agent_id));
  });

  it("deploys as before when the policy is off, even with one approver", () => {
    mount("normal", withPolicy(false, 1));
    setUpThrough(newAgent(probed().ws));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    const setUp = only();
    expect(setUp.deployed?.phase).toBe("recorded");
    expect(setUp.deployed?.rule).toBeUndefined();
    expect(probed().ws.agents.at(-1)).toMatchObject({ agent_id: setUp.agentId, label: "Agent 4" });
  });

  it.each([
    ["unreachable", "undelivered"],
    ["result-unknown", "unknown"],
  ] as const)("creates no version in the %s scenario, says %s, and goes no further", (scenario, phase) => {
    mount(scenario);
    act(() => {
      probed().createVersion(newAgent(probed().ws));
    });
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    expect(only().created.phase).toBe(phase);
    expect(only().created.validated).toBe(false);
    expect(only().confirmed).toBeNull();
    expect(probed().ws.agents.some((a) => a.label === "Agent 4")).toBe(false);
  });
});
