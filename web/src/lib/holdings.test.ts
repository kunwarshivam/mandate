import { describe, expect, it } from "vitest";
import { SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { add, dec, toFixed } from "./decimal";
import { CASH_KEY, UNMANAGED_KEY, accountSlices, agentSlices, assetTones, mandateUniverse } from "./holdings";

const WITH_AGENTS = SCENARIOS.map((s) => s.id).filter((id) => buildWorkspace(id).agents.length > 0);

describe("what the account holds", () => {
  it.each(WITH_AGENTS)("%s: the slices add up to the broker's equity, and the shares to one", (scenario) => {
    const ws = buildWorkspace(scenario);
    const { total, slices } = accountSlices(ws);
    expect(total).toBe(toFixed(dec(ws.connection.account_equity), 2));
    expect(toFixed(add(...slices.map((s) => dec(s.value))), 2)).toBe(total);
    expect(slices.reduce((sum, s) => sum + Number(s.share), 0)).toBeCloseTo(1, 3);
  });

  it("lists the held instruments largest first, then the agents' cash, then what no agent manages", () => {
    const { slices } = accountSlices(buildWorkspace("normal"));
    const assets = slices.filter((s) => s.kind === "asset");
    expect(assets.map((s) => Number(s.value))).toEqual([...assets.map((s) => Number(s.value))].sort((a, b) => b - a));
    expect(slices.slice(-2).map((s) => s.key)).toEqual([CASH_KEY, UNMANAGED_KEY]);
    expect(assets.map((s) => s.tone)).toEqual(["series-1", "series-2", "series-3"]);
  });

  it("gives an instrument the same colour across the account and in each agent", () => {
    const ws = buildWorkspace("normal");
    const tones = assetTones(ws);
    const account = new Map(accountSlices(ws).slices.map((s) => [s.key, s.tone]));
    for (const agent of ws.agents) {
      const slices = agentSlices(agent, tones);
      expect(toFixed(add(...slices.map((s) => dec(s.value))), 2)).toBe(toFixed(dec(agent.state.equity), 2));
      for (const s of slices.filter((x) => x.kind === "asset")) expect(s.tone).toBe(account.get(s.key));
    }
  });

  it("names what each mandate lets its agent trade, and which of those it holds", () => {
    for (const agent of buildWorkspace("normal").agents) {
      const { instruments } = mandateUniverse(agent);
      expect(instruments.map((i) => i.symbol)).toEqual(agent.mandate.universe.pinned_instruments.map((i) => i.symbol));
      for (const i of instruments) expect(i.held).toBe(agent.positions.some((p) => p.instrument.symbol === i.symbol));
    }
  });
});
