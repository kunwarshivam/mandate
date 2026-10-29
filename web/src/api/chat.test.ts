import { describe, expect, it } from "vitest";
import { chatRead } from "./chat";

/** The chat's answer: the model's reply always, and a card only when every typed member reads. */
const XYZ = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const ok = (card: unknown) => chatRead({ reply: { text: "Sure.", author: "agent" }, card }, "");

describe("the chat's answer (builder 4's route)", () => {
  it("reads the reply and each typed card", () => {
    expect(ok(null)).toEqual({ ok: true, value: { reply: "Sure.", card: null } });
    expect(ok({ action: "ask_to_buy", summary: { instrument_id: XYZ, symbol: "XYZ", size: "10" } })).toEqual({ ok: true, value: { reply: "Sure.", card: { action: "ask_to_buy", instrument_id: XYZ, symbol: "XYZ", size: "10" } } });
    expect(ok({ action: "pause", summary: { agent_id: "agt_1" } })).toMatchObject({ value: { card: { action: "pause" } } });
    expect(ok({ action: "owner_exit", summary: { instruments: [{ instrument_id: XYZ, symbol: "XYZ", qty: "8" }] } })).toMatchObject({ value: { card: { action: "owner_exit", instruments: [{ qty: "8" }] } } });
    expect(ok({ action: "create_agent", summary: { mandate_version: `sha256:${"ab".repeat(32)}` } })).toMatchObject({ value: { card: { action: "create_agent" } } });
  });

  it("drops a card it cannot read whole, and keeps the reply", () => {
    for (const card of [
      { action: "place_order", summary: {} },
      { action: "ask_to_buy", summary: { instrument_id: "AAPL", symbol: "AAPL", size: "10" } },
      { action: "ask_to_buy", summary: { instrument_id: XYZ, symbol: "XYZ", size: 10 } },
      { action: "ask_to_buy", summary: { instrument_id: XYZ, symbol: "XYZ", size: "0" } },
      { action: "ask_to_buy", summary: { instrument_id: XYZ, symbol: "XYZ", size: "-1" } },
      { action: "owner_exit", summary: { instruments: [] } },
      { action: "create_agent", summary: { mandate_version: "v1" } },
      { action: "pause" },
    ])
      expect(ok(card), JSON.stringify(card)).toEqual({ ok: true, value: { reply: "Sure.", card: null } });
  });

  it("refuses an answer with no reply text", () => {
    expect(chatRead({ card: null }, "")).toMatchObject({ ok: false });
    expect(chatRead({ reply: { text: 4 } }, "")).toMatchObject({ ok: false });
  });
});
