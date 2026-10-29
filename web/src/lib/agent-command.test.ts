import { describe, expect, it } from "vitest";
import { EXAMPLES, readCommand } from "./agent-command";

/** The owner's words to an agent, each read as exactly one action or as nothing (no model). */
describe("reading what the owner types to an agent", () => {
  it.each([
    ["buy 10 AAPL", { kind: "ask_buy", size: "10", symbol: "AAPL" }],
    ["Buy 2.5 aapl", { kind: "ask_buy", size: "2.5", symbol: "AAPL" }],
    ["please buy 10 shares of spy", { kind: "ask_buy", size: "10", symbol: "SPY" }],
    ["ask it to buy 3 QQQ.", { kind: "ask_buy", size: "3", symbol: "QQQ" }],
    ["buy XYZ 4", { kind: "ask_buy", size: "4", symbol: "XYZ" }],
    ["buy 1 btc/usd", { kind: "ask_buy", size: "1", symbol: "BTC/USD" }],
    ["pause", { kind: "pause" }],
    ["Pause it", { kind: "pause" }],
    ["resume", { kind: "resume" }],
    ["stop", { kind: "stop" }],
    ["kill switch", { kind: "kill" }],
    ["sell everything", { kind: "exit", symbol: null }],
    ["exit", { kind: "exit", symbol: null }],
    ["exit AAPL", { kind: "exit", symbol: "AAPL" }],
    ["sell all of xyz", { kind: "exit", symbol: "XYZ" }],
    ["how am I doing?", { kind: "status" }],
    ["positions", { kind: "status" }],
    ["should I buy AAPL?", { kind: "advice" }],
    ["what should i buy", { kind: "advice" }],
  ] as const)("%s", (said, expected) => {
    expect(readCommand(said)).toEqual(expected);
  });

  it.each(["", "hello", "buy AAPL", "buy -5 AAPL", "buy 0x10 AAPL", "sell 5 AAPL", "buy 10", "stop buying", "pause and buy 10 AAPL", "buy 10 AAPL and 5 SPY"])("reads %j as nothing it can do", (said) => {
    expect(readCommand(said).kind).toBe("unknown");
  });

  it("reads every example it offers as an action", () => {
    for (const example of EXAMPLES) expect(["unknown", "advice"], example).not.toContain(readCommand(example).kind);
  });
});
