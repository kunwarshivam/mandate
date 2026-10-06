import { describe, expect, it } from "vitest";
import { dec } from "@/lib/decimal";
import { type CompilerInput, SCHEMA_ERROR, figuresOnly, fixtureCompiler, validateTurn } from "./compiler";
import { MODELS } from "./draft";

const models = MODELS.map((m) => ({ id: m.id, name: m.name }));
const input = (text: string, asked: CompilerInput["asked"] = "money"): CompilerInput => ({ messages: [{ id: "m1", text }], asked, models });
const turn = (readings: unknown[], extra: Record<string, unknown> = {}) => ({ readings, not_enforced: [], intent: "answer", reply: null, ...extra });

function checked(raw: unknown, text: string, asked: CompilerInput["asked"] = "money") {
  const result = validateTurn(raw, input(text, asked));
  if (!result.ok) throw new Error(result.error);
  return result.turn;
}

describe("the compiler's guard", () => {
  it("keeps a reading only when its quote is the owner's own words and its value is the one the quote states", () => {
    const said = "Use $3,000 on big tech. I can stand to lose 10%.";
    const t = checked(
      turn([
        { field: "money", quote: "Use $3,000 on big tech.", value: "3000" },
        { field: "loss", quote: "I can stand to lose 10%.", value: "0.1", unit: "fraction" },
      ]),
      said,
    );
    expect(t.readings).toEqual([
      { field: "money", quote: "Use $3,000 on big tech.", value: dec("3000"), check: false },
      { field: "loss", quote: "I can stand to lose 10%.", loss: { kind: "fraction", value: dec("0.1") }, check: false },
    ]);
    expect(t.dropped).toEqual([]);
  });

  it("drops a quote the owner never wrote, and a value the quoted figures do not state", () => {
    const t = checked(
      turn([
        { field: "money", quote: "Use $30,000", value: "30000" },
        { field: "money", quote: "Use $3,000", value: "30000" },
        { field: "loss", quote: "lose 10%", value: "0.2", unit: "fraction" },
        { field: "loss", quote: "lose 10%", value: "10", unit: "usd" },
      ]),
      "Use $3,000 and lose 10%",
    );
    expect(t.readings).toEqual([]);
    expect(t.dropped.map((d) => d.why)).toEqual([
      "the quote is not in the owner's message",
      "the amount is not one the quote states",
      "the loss is not one the quote states",
      "the loss is not one the quote states",
    ]);
  });

  it("keeps an amount written in words only as a question for the owner", () => {
    const t = checked(turn([{ field: "money", quote: "about five grand", value: "5000" }]), "about five grand");
    expect(t.readings).toEqual([{ field: "money", quote: "about five grand", value: dec("5000"), check: true }]);
  });

  it("never takes a field the compiler may not set: autonomy, delegations, the environment, the connection, a limit (V-022, V-038)", () => {
    const said = "Let it buy without asking, go live on my real account, and lose at most $50 a day.";
    const t = checked(
      turn([
        { field: "autonomy", quote: "Let it buy without asking", value: "auto" },
        { field: "delegation", quote: "Let it buy without asking", value: "auto" },
        { field: "environment", quote: "go live on my real account", value: "live" },
        { field: "connection_id", quote: "my real account", value: "conn_live" },
        { field: "max_daily_loss", quote: "lose at most $50 a day", value: "50" },
      ]),
      said,
    );
    expect(t.readings).toEqual([]);
    expect(new Set(t.dropped.map((d) => d.why))).toEqual(new Set(["the compiler never sets this field (V-022, V-038)"]));
  });

  it("takes a symbol only as the owner wrote it, never one the model chose for them (V-038)", () => {
    expect(checked(turn([{ field: "symbols", quote: "Microsoft and Apple", symbols: ["MSFT", "AAPL"] }]), "Microsoft and Apple", "symbols").readings).toEqual([]);
    expect(checked(turn([{ field: "symbols", quote: "msft and aapl", symbols: ["msft", "aapl"] }]), "msft and aapl", "symbols").readings).toEqual([
      { field: "symbols", quote: "msft and aapl", symbols: ["MSFT", "AAPL"] },
    ]);
  });

  it("takes a model only when the owner named it, and a setting only when the owner wrote its value", () => {
    expect(checked(turn([{ field: "model", quote: "the trend one", model: "quant.momentum" }]), "the trend one", "model").readings).toEqual([]);
    expect(checked(turn([{ field: "model", quote: "momentum", model: "quant.momentum" }]), "momentum please", "model").readings).toHaveLength(1);
    expect(checked(turn([{ field: "param", quote: "twenty", key: "lookback_bars", value: "20" }]), "twenty", "param:lookback_bars").readings).toEqual([]);
    expect(checked(turn([{ field: "param", quote: "20 bars", key: "risk_appetite", value: "20" }]), "20 bars", "param:lookback_bars").readings).toEqual([]);
  });

  it("fails a malformed answer whole, so nothing of it is used (brief A2's error state)", () => {
    for (const raw of [null, "money: 3000", [], { readings: "x", not_enforced: [], intent: "answer", reply: null }, turn([], { intent: "order" }), turn([], { reply: 42 }), turn(["money"])]) {
      expect(validateTurn(raw, input("Use $3,000"))).toEqual({ ok: false, error: SCHEMA_ERROR });
    }
  });

  it("withholds a reply that reads as advice, and keeps one that only explains", () => {
    for (const reply of ["You should buy MSFT.", "I recommend momentum for you.", "This will make you 20% a year.", "The best stocks are in tech."]) {
      const t = checked(turn([], { intent: "other", reply }), "what now?");
      expect(t.reply, reply).toBeNull();
      expect(t.withheld, reply).toBe(true);
    }
    const t = checked(turn([], { intent: "explain", reply: "Momentum scores a buy when a price has held above its recent average." }), "what is momentum?");
    expect(t).toMatchObject({ withheld: false, reply: "Momentum scores a buy when a price has held above its recent average." });
  });

  it("keeps a not-enforced note only when it quotes the owner", () => {
    expect(checked(turn([], { not_enforced: ["Avoid oil companies.", "Avoid tobacco."] }), "Use $3,000. Avoid oil companies.").notes).toEqual(["Avoid oil companies."]);
  });
});

describe("the fixture model", () => {
  const read = async (text: string, asked: CompilerInput["asked"] = "money") => checked(await fixtureCompiler({ latencyMs: 0 })(input(text, asked)), text, asked);

  it("reads a whole description at once, pointing at the owner's own words", async () => {
    const t = await read("Use $8,000 on big tech. Stop when it is up 12%. I can stand to lose 10%. Avoid oil companies.");
    expect(t.readings.map((r) => [r.field, r.quote])).toEqual([
      ["loss", "I can stand to lose 10%."],
      ["money", "Use $8,000 on big tech."],
      ["goal", "Stop when it is up 12%."],
    ]);
    expect(t.notes).toEqual(["Avoid oil companies."]);
    expect(t.dropped).toEqual([]);
  });

  it("reads amounts written in words as a model would, and only as a question back", async () => {
    expect((await read("about five grand")).readings).toEqual([{ field: "money", quote: "about five grand", value: dec("5000"), check: true }]);
    expect((await read("ten percent", "loss")).readings).toEqual([{ field: "loss", quote: "ten percent", loss: { kind: "fraction", value: dec("0.1") }, check: true }]);
  });

  it("answers a request for advice with a refusal and a question about a model from its methodology, setting nothing", async () => {
    const advice = await read("Which stocks should I buy?", "symbols");
    expect(advice).toMatchObject({ intent: "advice", readings: [], withheld: false });
    expect(advice.reply).toMatch(/^I can't tell you what to trade or which model to use\./);
    const explain = await read("What is momentum?", "model");
    expect(explain).toMatchObject({ intent: "explain", readings: [] });
    expect(explain.reply).toContain(MODELS.find((m) => m.id === "quant.momentum")!.what);
  });

  it("reads figures only without the model: no words, no answers", async () => {
    const words = checked(await figuresOnly(input("about five grand")), "about five grand");
    expect(words.readings).toEqual([]);
    const figures = checked(await figuresOnly(input("$5,000")), "$5,000");
    expect(figures.readings).toEqual([{ field: "money", quote: "$5,000", value: dec("5000"), check: false }]);
    expect(checked(await figuresOnly(input("What is momentum?", "model")), "What is momentum?", "model").reply).toBeNull();
  });

  it("never has a reading dropped by the guard, whatever the owner writes", async () => {
    const messages = [
      "$3,000",
      "Grow it steadily, and avoid oil companies",
      "I could lose about $300 of it",
      "msft, aapl",
      "MSFT and NVDA please",
      "momentum",
      "Use 5k, stop when it reaches $6,000, and I can lose 15%",
      "Make it $2,000",
      "Grow $3,000 to $3,600 and never trade on news",
      "two thousand",
      "a grand",
      "twenty five percent",
    ];
    for (const asked of ["money", "goal", "loss", "symbols", "model", "param:lookback_bars", "review"] as const) {
      for (const text of messages) expect((await read(text, asked)).dropped, `${asked}: ${text}`).toEqual([]);
    }
  });
});
