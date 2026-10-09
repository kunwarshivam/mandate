import { describe, expect, it } from "vitest";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { type AskContext, type Reply, STARTERS, interpret } from "./ask-record";
import { editableFields, readInput, valueAt } from "./mandate-change";
import { changeLabel } from "./mandate-paths";

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

  it("dates a restriction that began on an earlier day when it says an agent's status (C-23)", () => {
    const said = lines(interpret("What is its status?", ctx(AGENT_IDS.btc, "drawdown")));
    expect(said).toContain("Since Sep 26, 2026, 15:12: Drawdown: sizes scaled.");
    expect(said).toContain("Since 14:01:12: Drawdown: selling only.");
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

describe("changing limits from a message (DEC-483)", () => {
  const change = (said: string, c: AskContext = ctx(AGENT_IDS.swing)) => {
    const reply = interpret(said, c);
    if (reply.kind !== "change") throw new Error(`expected a change, got ${JSON.stringify(reply)}`);
    return reply;
  };

  it("reads one limit and its new value into the change review, with the owner's words", () => {
    expect(interpret("Set the largest order to $800", ctx(AGENT_IDS.swing))).toEqual({
      kind: "change",
      agentId: AGENT_IDS.swing,
      edits: { "/risk/max_order_usd": "800" },
      quote: "Set the largest order to $800",
    });
  });

  it("reads several clauses, each in its field's unit", () => {
    expect(change("lower the largest order to 800 and the daily loss limit to 1.5%, set the approval window to 15 minutes please").edits).toEqual({
      "/risk/max_order_usd": "800",
      "/risk/max_daily_loss": "0.015",
      "/autonomy/approval/timeout_s": 900,
    });
    expect(change("set orders a day to 20; the protective stop to 6 percent").edits).toEqual({ "/risk/max_orders_per_day": 20, "/protection/stop_distance": "0.06" });
    expect(change("approval window to 1 hour").edits).toEqual({ "/autonomy/approval/timeout_s": 3600 });
    expect(change("set capital to $9,500 for agent 2", ctx()).edits).toEqual({ "/capital/allocation_usd": "9500" });
  });

  it("reads the largest position as a share when the value is a percentage, else in dollars", () => {
    expect(change("set the largest position to 30% of equity").edits).toEqual({ "/risk/max_position_fraction": "0.3" });
    expect(change("set the largest position to $1,200").edits).toEqual({ "/risk/max_position_usd": "1200" });
  });

  it("reads both quiet hours, a removed second approver, and the stop as a limit rather than Stop", () => {
    expect(change("set quiet hours between 22:00 and 6:30").edits).toEqual({ "/notifications/quiet_hours/start": "22:00", "/notifications/quiet_hours/end": "06:30" });
    expect(change("remove the two-approver threshold", ctx(AGENT_IDS.lmn)).edits).toEqual({ "/autonomy/approval/two_approver_above_usd": null });
    expect(change("set the stop to 6%").edits).toEqual({ "/protection/stop_distance": "0.06" });
  });

  it("reads every editable field written as the form writes it, to the same value the form reads", () => {
    for (const agent of ctx().ws.agents) {
      for (const f of editableFields(agent.mandate)) {
        const now = valueAt(agent.mandate, f.path);
        const typed = f.path === "/autonomy/approval/two_approver_above_usd" ? "750" : f.unit === "time" ? "21:15" : f.unit === "count" || f.unit === "minutes" ? "37" : "1.25";
        const read = readInput(f, typed);
        if (!read.ok || read.value === now) throw new Error(`pick another value for ${f.path}`);
        const said = `set ${changeLabel(f.path).toLowerCase().replace(",", "")} to ${typed}${f.unit === "percent" ? "%" : ""}`;
        expect(change(said, ctx(agent.agent_id)).edits, said).toEqual({ [f.path]: read.value });
      }
    }
  });

  it("finds the agent named in the copilot, and asks which when none is", () => {
    expect(change("Set Agent 3's largest order to 500", ctx()).agentId).toBe(AGENT_IDS.lmn);
    expect(lines(interpret("set the largest order to 500", ctx()))).toEqual(["Which agent? Name it, for example “set Agent 1's largest order to $800”."]);
  });

  it.each([
    ["raise the largest order by $200", "Largest order: say the new value rather than the difference, like “to $800”, not “by $200”."],
    ["set the largest order to eight hundred", "Largest order: write the new value in figures, like $800 or 1.5%."],
    ["set capital to 2k", "Capital: write the whole amount, like 2,000."],
    ["set the daily loss limit to $200", "Daily loss limit: write a percentage in figures, like 2 or 2.5."],
    ["set quiet hours to 22:00", "Quiet hours: say both times on a 24-hour clock, like “quiet hours 22:00 to 07:00”."],
    ["set the largest order to 800 and the largest order to 700", "Largest order is named twice. Say it once, with the value you want."],
    ["set the largest order to 800 and the vibe to calm", "I couldn't tell which limit “the vibe to calm” is about."],
  ])("asks rather than guesses for %j", (said, line) => {
    const reply = interpret(said, ctx(AGENT_IDS.swing));
    expect(lines(reply)).toEqual([line]);
    expect(reply.kind === "answer" && reply.cites).toEqual([{ href: `/agents/${AGENT_IDS.swing}/mandate/edit`, label: "Edit Agent 2's mandate" }]);
  });

  it("says what a message can change when asked for something else", () => {
    const said = lines(interpret("change the instruments to AAPL", ctx(AGENT_IDS.swing)));
    expect(said[0]).toBe("That part of Agent 2's mandate isn't changed from a message.");
    expect(said[1]).toMatch(/^A message or the Edit form changes these: capital/);
  });

  it.each(["should I raise my capital to 20000?", "What should my daily loss limit be?", "is it a good idea to lower the largest order to 500"])("suggests no limit for %j", (said) => {
    expect(lines(interpret(said, ctx(AGENT_IDS.swing)))[0]).toBe("I don't suggest limits: they're yours to set.");
  });

  it.each(["What is my largest order?", "Is the daily loss limit 2%?", "How close is it to its limits?"])("reads %j as a question", (said) => {
    expect(interpret(said, ctx(AGENT_IDS.swing)).kind).toBe("answer");
    expect(lines(interpret(said, ctx(AGENT_IDS.swing)))[0]).not.toBe("I don't suggest limits: they're yours to set.");
  });

  it("says when a value is already the mandate's, and that an agent without quiet hours has none to move", () => {
    expect(lines(interpret("set the largest order to $1,000.00", ctx(AGENT_IDS.swing)))).toEqual(["Largest order is already set to that. Nothing to change."]);
    const c = ctx(AGENT_IDS.swing);
    c.ws.agents.find((a) => a.agent_id === AGENT_IDS.swing)!.mandate.notifications.quiet_hours = null;
    expect(lines(interpret("set quiet hours 22:00 to 07:00", c))).toEqual(["Agent 2 has no quiet hours set, so there are none to move."]);
  });
});
