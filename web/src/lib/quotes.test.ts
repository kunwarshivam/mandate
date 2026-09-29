import { describe, expect, it } from "vitest";
import { buildMarket } from "@/fixtures/market";
import { SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { allFeedsOk, heldSymbols, oldestFeed, quotes } from "./quotes";

describe("quotes for the ticker tape", () => {
  it("gives each held instrument once, with its last price and the change since the previous close", () => {
    const ws = buildWorkspace("normal");
    expect(quotes(ws, buildMarket(ws))).toEqual([
      { symbol: "BTC/USD", last: "56789.01", change: "+1.07%", direction: "gain" },
      { symbol: "XYZ", last: "141.23", change: "−0.36%", direction: "loss" },
      { symbol: "QRS", last: "97.65", change: "−0.27%", direction: "loss" },
    ]);
  });

  it("follows the mark: a drawdown turns the day's change to a loss", () => {
    const ws = buildWorkspace("drawdown");
    expect(quotes(ws, buildMarket(ws))[0]).toEqual({ symbol: "BTC/USD", last: "51843.58", change: "−1.14%", direction: "loss" });
  });

  it("lists nothing when nothing is held, and skips a position closed to zero", () => {
    expect(heldSymbols(buildWorkspace("empty"))).toEqual([]);
    const ws = buildWorkspace("normal");
    ws.agents[0].positions = ws.agents[0].positions.map((p) => ({ ...p, qty: "0" }));
    expect(heldSymbols(ws).map((h) => h.symbol)).not.toContain(buildWorkspace("normal").agents[0].positions[0]?.instrument.symbol);
  });

  it("carries prices and percentages only, never an amount won or lost", () => {
    const ws = buildWorkspace("normal");
    for (const q of quotes(ws, buildMarket(ws))) expect(Object.keys(q).sort()).toEqual(["change", "direction", "last", "symbol"]);
  });

  it("names the feed heard from longest ago", () => {
    expect(oldestFeed(buildWorkspace("normal"))).toEqual({ key: "relay", as_of: "2026-09-28T14:05:10-04:00" });
  });

  it.each(SCENARIOS.map((s) => s.id))("in %s, allows quotes only while the workspace is ready and every feed answers", (scenario) => {
    const ws = buildWorkspace(scenario);
    expect(allFeedsOk(ws)).toBe(ws.status === "ready" && Object.values(ws.health).every((f) => f.state === "ok"));
  });

  it("keeps the strip for a stale feed, an unreachable deployment, or a workspace still loading", () => {
    for (const scenario of ["stale", "unreachable", "loading"] as const) expect(allFeedsOk(buildWorkspace(scenario)), scenario).toBe(false);
    expect(allFeedsOk(buildWorkspace("normal"))).toBe(true);
  });
});
