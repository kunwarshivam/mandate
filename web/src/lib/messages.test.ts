import { describe, expect, it } from "vitest";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { byDay, deskFor, inboxRows, itemLine, listStamp, stamp, threadItems } from "./messages";

describe("an agent's thread", () => {
  it("is every journaled event and request for the agent, oldest first", () => {
    const ws = buildWorkspace("normal");
    const items = threadItems(ws, AGENT_IDS.swing, ws.now);
    const times = items.map((i) => Date.parse(i.at));
    expect(times).toEqual([...times].sort((a, b) => a - b));
    expect(items.at(-1)).toMatchObject({ kind: "request", id: APPROVAL_IDS.swingXyz });
    expect(items.every((i) => (i.kind === "event" ? ws.timeline[AGENT_IDS.swing].includes(i.event) : i.approval.agent_id === AGENT_IDS.swing))).toBe(true);
  });

  it("shows a request once: the journal's own line for asking at the same time is not repeated", () => {
    const ws = buildWorkspace("normal");
    const items = threadItems(ws, AGENT_IDS.swing, ws.now);
    expect(items.some((i) => i.kind === "event" && i.event.text.startsWith("Asked you to approve"))).toBe(false);
    const btc = threadItems(ws, AGENT_IDS.btc, ws.now);
    expect(btc.filter((i) => i.kind === "event" && i.event.kind === "approval")).toEqual([]);
    expect(btc.filter((i) => i.kind === "request").map((i) => i.id)).toEqual(["apr_01JBDJ8FSSZ5AWSH8VHTPRE95B", "apr_01JBQY77ZXYYK596NGYA1DQ91K"]);
  });

  it("keeps events of other kinds that share a request's time", () => {
    const ws = buildWorkspace("normal");
    const events = threadItems(ws, AGENT_IDS.swing, ws.now).filter((i) => i.kind === "event");
    expect(events).toHaveLength(ws.timeline[AGENT_IDS.swing].length - 1);
  });

  it("reads a request past its deadline as skipped, not waiting", () => {
    const ws = buildWorkspace("normal");
    const later = "2026-09-28T14:20:00-04:00";
    const request = threadItems(ws, AGENT_IDS.swing, later).find((i) => i.id === APPROVAL_IDS.swingXyz);
    expect(request).toMatchObject({ kind: "request", approval: { status: "expired" } });
  });

  it("is empty for an agent with nothing journaled", () => {
    const ws = buildWorkspace("normal");
    ws.timeline = {};
    ws.approvals = [];
    expect(threadItems(ws, AGENT_IDS.lmn, ws.now)).toEqual([]);
  });

  it("groups by calendar day, with today named", () => {
    const ws = buildWorkspace("normal");
    const groups = byDay(threadItems(ws, AGENT_IDS.btc, ws.now), ws.now);
    expect(groups.map((g) => g.label)).toEqual(["Sep 24, 2026", "Today"]);
    expect(groups.flatMap((g) => g.items)).toEqual(threadItems(ws, AGENT_IDS.btc, ws.now));
  });

  it("stamps today's items with the clock alone", () => {
    const ws = buildWorkspace("normal");
    expect(stamp(ws.now, ws.now)).toMatch(/^\d{2}:\d{2}:\d{2}$/);
    expect(stamp("2026-09-24T09:44:31-04:00", ws.now)).toBe("Sep 24, 2026, 09:44:31");
  });
});

describe("the inbox", () => {
  it("puts agents with a request waiting first, soonest deadline first", () => {
    const ws = buildWorkspace("approvals");
    const { needsYou, earlier } = inboxRows(ws, ws.now);
    expect(needsYou.map((r) => r.agent.agent_id)).toEqual([AGENT_IDS.btc, AGENT_IDS.swing, AGENT_IDS.lmn]);
    expect(earlier).toEqual([]);
  });

  it("puts every other agent after, latest item first", () => {
    const ws = buildWorkspace("normal");
    const { needsYou, earlier } = inboxRows(ws, ws.now);
    expect(needsYou.map((r) => r.agent.agent_id)).toEqual([AGENT_IDS.swing]);
    expect(earlier.map((r) => r.agent.agent_id)).toEqual([AGENT_IDS.btc, AGENT_IDS.lmn]);
    expect(needsYou.length + earlier.length).toBe(ws.agents.length);
  });

  it("drops a request from Needs you once its deadline passes", () => {
    const ws = buildWorkspace("normal");
    expect(inboxRows(ws, "2026-09-28T14:20:00-04:00").needsYou).toEqual([]);
  });

  it("describes the latest item in one line", () => {
    const ws = buildWorkspace("normal");
    const [row] = inboxRows(ws, ws.now).needsYou;
    expect(row.last && itemLine(row.last)).toBe("Asks you to approve buy 2 XYZ at $141.30.");
    const resolved = threadItems(ws, AGENT_IDS.btc, ws.now).find((i) => i.kind === "request");
    expect(resolved && itemLine(resolved)).toBe("Request to buy 0.015 BTC/USD at $55,400.00: skipped at the deadline.");
  });
});

describe("the desk", () => {
  it("walks the open request from the models to you, each step from the record", () => {
    const ws = buildWorkspace("normal");
    const desk = deskFor(ws, AGENT_IDS.swing, ws.now);
    expect(desk?.subject).toBe("request");
    expect(desk?.steps.map((s) => s.role)).toEqual(["research", "trader", "risk", "autonomy", "you"]);
    const [research, trader, risk, rules, you] = desk?.steps ?? [];
    const approval = ws.approvals.find((a) => a.approval_id === APPROVAL_IDS.swingXyz);
    expect(research.quotes).toEqual(approval?.evidence);
    expect(research.at).toBe("2026-09-28T14:04:51-04:00");
    expect(trader.at).toBeNull();
    expect(trader.lines[0]).toBe("Buy 2 XYZ at a limit of $141.30. Purpose: increase.");
    expect(risk).toMatchObject({ at: "2026-09-28T14:04:58-04:00", lines: ["Allowed."], href: `/agents/${AGENT_IDS.swing}/decisions/01JB5GQAPENECFSECZP11HYGCP` });
    expect(risk.figures).toEqual(approval?.risk_impact);
    expect(rules.lines).toEqual([approval?.trigger]);
    expect(you).toMatchObject({ lines: ["Waiting for you."], href: `/approvals/${APPROVAL_IDS.swingXyz}` });
  });

  it("quotes model output only in the research step", () => {
    const ws = buildWorkspace("normal");
    const steps = deskFor(ws, AGENT_IDS.swing, ws.now)?.steps ?? [];
    expect(steps.filter((s) => s.quotes.length > 0).map((s) => s.role)).toEqual(["research"]);
  });

  it("says the gate's decision is not recorded when it is missing, and links nowhere", () => {
    const ws = buildWorkspace("normal");
    ws.decisions = [];
    const risk = deskFor(ws, AGENT_IDS.swing, ws.now)?.steps.find((s) => s.role === "risk");
    expect(risk).toMatchObject({ at: null, href: null, lines: ["The gate's decision is not recorded here."] });
  });

  it("ends a resolved request with what happened", () => {
    const ws = buildWorkspace("normal");
    const you = deskFor(ws, AGENT_IDS.btc, ws.now)?.steps.at(-1);
    expect(you).toMatchObject({ role: "you", lines: ["Skipped by you. Nothing was sent."], at: "2026-09-28T11:20:03-04:00" });
  });

  it("falls back to the latest gate decision with no request, and to nothing with neither", () => {
    const ws = buildWorkspace("normal");
    ws.approvals = [];
    const desk = deskFor(ws, AGENT_IDS.lmn, ws.now);
    expect(desk).toMatchObject({ subject: "decision", title: "Buy 11 LMN at a limit of $45.10, not allowed" });
    expect(desk?.steps.find((s) => s.role === "risk")?.lines[0]).toBe("Not allowed: No re-entry within 1 h of an exit in the same instrument.");
    ws.decisions = [];
    expect(deskFor(ws, AGENT_IDS.lmn, ws.now)).toBeNull();
  });
});

describe("thread list times", () => {
  it("are hours and minutes today, and the month and day before", () => {
    const ws = buildWorkspace("normal");
    expect(listStamp("2026-09-28T09:05:00-04:00", ws.now)).toBe("09:05");
    expect(listStamp("2026-09-24T09:44:31-04:00", ws.now)).toBe("Sep 24");
  });
});
