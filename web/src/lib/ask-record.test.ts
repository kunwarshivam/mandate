import { describe, expect, it } from "vitest";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { type AskContext, type Reply, STARTERS, interpret } from "./ask-record";

function ctx(agentId: string | null = null, scenario: Parameters<typeof buildWorkspace>[0] = "normal"): AskContext {
  const ws = buildWorkspace(scenario);
  return { ws, now: ws.now, agentId };
}

function lines(reply: Reply): string[] {
  if (reply.kind !== "answer") throw new Error(`expected an answer, got ${reply.kind}`);
  return reply.lines;
}

describe("asking the record", () => {
  it("lists what needs you, by deadline, with a link to each request", () => {
    const reply = interpret("What needs me?", ctx(null, "approvals"));
    expect(lines(reply)[0]).toBe("3 requests are waiting for you.");
    expect(reply.kind === "answer" && reply.cites.map((c) => c.href)).toEqual([
      `/approvals/${APPROVAL_IDS.btc}`,
      `/approvals/${APPROVAL_IDS.swingXyz}`,
      `/approvals/${APPROVAL_IDS.lmn}`,
    ]);
  });

  it("answers within the thread's agent only", () => {
    const reply = interpret("anything waiting?", ctx(AGENT_IDS.lmn, "approvals"));
    expect(lines(reply)).toEqual(["One request is waiting for you.", "Agent 3: buy 10 LMN at $45.67. Skipped at 14:16:05 if you do nothing."]);
  });

  it("explains why an agent asked from its rules and the gate, citing both", () => {
    const reply = interpret("Why did it ask?", ctx(AGENT_IDS.swing));
    const said = lines(reply);
    expect(said[0]).toBe("Agent 2's latest: Buy 2 XYZ at a limit of $141.30, waiting for you.");
    expect(said).toContain("Your rule \u201clow_score\u201d: ask when the combined model score is below 0.65.");
    expect(said).toContain("Risk gate: Allowed.");
    expect(said).toContain("Position after it fills: $1,412.44 of the $1,500.00 position limit");
    expect(reply.kind === "answer" && reply.cites).toEqual([
      { href: `/agents/${AGENT_IDS.swing}/decisions/01JB5GQAPENECFSECZP11HYGCP`, label: "Gate decision" },
      { href: `/agents/${AGENT_IDS.swing}/mandate`, label: "The rule in the mandate" },
      { href: `/approvals/${APPROVAL_IDS.swingXyz}`, label: "The request" },
    ]);
  });

  it("never repeats model output as its own words", () => {
    const c = ctx(AGENT_IDS.swing);
    const evidence = c.ws.approvals.flatMap((a) => a.evidence.flatMap((e) => e.lines));
    for (const q of [...STARTERS, "why", "what happened today"]) {
      const reply = interpret(q, c);
      if (reply.kind === "answer") for (const line of reply.lines) expect(evidence.some((e) => line.includes(e))).toBe(false);
    }
  });

  it("asks which agent when a why question has more than one", () => {
    const reply = interpret("why?", ctx());
    expect(reply.kind === "answer" && reply.follow).toEqual(["Why did Agent 1 ask?", "Why did Agent 2 ask?", "Why did Agent 3 ask?"]);
  });

  it("finds an agent named in the message", () => {
    const reply = interpret("Why did Agent 1 ask?", ctx());
    expect(lines(reply)[0]).toBe("Agent 1's latest: Buy 0.01 BTC/USD at a limit of $56,100.00, skipped by you.");
  });

  it("gives limits as distances and caps, never as results", () => {
    const reply = interpret("How close is it to its limits?", ctx(AGENT_IDS.swing));
    const said = lines(reply);
    expect(said[0]).toMatch(/^Agent 2: \$[\d,.]+ above its /);
    expect(said.at(-1)).toMatch(/^Orders today: \d+ of \d+\.$/);
    const table = reply.kind === "answer" ? reply.table : undefined;
    expect(table?.columns).toEqual(["Limit", "Headroom", "Set at", "At the limit"]);
    const daily = table?.rows.find((r) => r[0] === "Daily loss limit");
    expect(daily?.[1]).toMatch(/^\$[\d,.]+$/);
    expect(daily?.[2]).toMatch(/^\$[\d,.]+ below the day's start$/);
    expect(daily?.[3]).not.toBe("");
    for (const row of table?.rows ?? []) expect(row).toHaveLength(4);
    expect([...said, ...(table?.rows.flat() ?? [])].join(" ")).not.toMatch(/profit|P&L|return|gain/i);
  });

  it("names the agent on each row when it gives every agent's limits", () => {
    const reply = interpret("How close are my agents to their limits?", ctx());
    const table = reply.kind === "answer" ? reply.table : undefined;
    expect(table?.columns[0]).toBe("Agent");
    expect(new Set(table?.rows.map((r) => r[0])).size).toBeGreaterThan(1);
  });

  it("names holdings with the time of each mark", () => {
    const said = lines(interpret("what does it hold", ctx(AGENT_IDS.swing)));
    expect(said.length).toBeGreaterThan(0);
    for (const line of said) expect(line).toMatch(/^Agent 2: [\d.,]+ [A-Z/]+, worth \$[\d,.]+ at the \d{2}:\d{2}:\d{2} mark\.$/);
  });

  it("reads today's activity newest first", () => {
    const said = lines(interpret("What happened today?", ctx(AGENT_IDS.btc)));
    expect(said[0]).toBe("13:40:02: Buy 0.02 BTC/USD not allowed: larger than the $1,000.00 order limit.");
    expect(said.every((l) => /^\d{2}:\d{2}:\d{2}: /.test(l))).toBe(true);
  });

  it("explains the latest refusal with the rule", () => {
    expect(lines(interpret("why was it denied?", ctx(AGENT_IDS.btc)))).toEqual(["At 13:40:02, Agent 1: buy 0.02 BTC/USD. Not allowed: Orders are at most $1,000.00."]);
  });

  it("says what it can answer when nothing matches, without guessing", () => {
    const reply = interpret("tell me a joke", ctx());
    expect(lines(reply)).toEqual(["I can't answer that one. Try one of these."]);
    expect(reply.kind === "answer" && reply.follow).toEqual([...STARTERS]);
  });
});

describe("acting from a message", () => {
  it("offers Pause only when the owner asks to pause", () => {
    expect(interpret("pause it", ctx(AGENT_IDS.swing))).toEqual({ kind: "pause", agentId: AGENT_IDS.swing });
    expect(interpret("Please pause Agent 3", ctx())).toEqual({ kind: "pause", agentId: AGENT_IDS.lmn });
    for (const q of STARTERS) expect(interpret(q, ctx(AGENT_IDS.swing)).kind).toBe("answer");
  });

  it("reads a question about pausing as a question", () => {
    expect(interpret("Is Agent 2 paused?", ctx()).kind).toBe("answer");
    expect(interpret("why did it pause", ctx(AGENT_IDS.swing)).kind).toBe("answer");
  });

  it("asks which agent to pause when the message names none", () => {
    const reply = interpret("pause", ctx());
    expect(reply.kind === "answer" && reply.follow).toEqual(["Pause Agent 1", "Pause Agent 2", "Pause Agent 3"]);
  });

  it("does not offer to pause an agent that is already paused", () => {
    expect(lines(interpret("pause it", ctx(AGENT_IDS.swing, "paused")))).toEqual(["Agent 2 is already paused."]);
  });

  it("opens Stop for stopping, the kill switch and resuming, which need a passkey", () => {
    expect(interpret("stop agent 2", ctx())).toEqual({ kind: "stop", agentId: AGENT_IDS.swing, resume: false });
    expect(interpret("kill everything", ctx())).toEqual({ kind: "stop", agentId: null, resume: false });
    expect(interpret("resume it", ctx(AGENT_IDS.swing, "paused"))).toEqual({ kind: "stop", agentId: AGENT_IDS.swing, resume: true });
  });

  it.each(["buy 10 XYZ", "sell my BTC", "Should I buy XYZ?", "Is it a good time to buy?", "place an order for LMN"])("places no order and gives no advice for %j", (said) => {
    expect(lines(interpret(said, ctx(AGENT_IDS.swing)))[0]).toBe("Orders aren't placed from a message, and I don't suggest trades.");
  });

  it("hands creating an agent to setup with the owner's words", () => {
    expect(interpret("Create an agent that buys BTC weekly with $500", ctx())).toEqual({ kind: "create", text: "Create an agent that buys BTC weekly with $500" });
  });

  it.each(["Create an agent", "can you create a new agent please", "I want another agent"])("hands nothing to setup when %j says nothing about the agent", (said) => {
    expect(interpret(said, ctx())).toEqual({ kind: "create", text: null });
  });
});
