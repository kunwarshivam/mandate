import { describe, expect, it } from "vitest";
import { instrumentsRead, signalModelsRead } from "@/api/catalogue";
import { MOCK_MODEL } from "@/api/mock-server";
import { constraintBroken, readStrategy, servedModelChoice } from "./draft";

const SERVED = {
  model_id: MOCK_MODEL.model_id,
  model_version: MOCK_MODEL.model_version,
  content_hash: MOCK_MODEL.content_hash,
  params: [...MOCK_MODEL.params],
  admits_instruments: false,
  content: { methodology: "How it decides.", params: [{ name: "fast_periods", type: "integer", min: 1, max: 1000 }, { name: "slow_periods", type: "integer", min: 2, max: 1000 }], constraints: ["fast_periods < slow_periods"] },
};

describe("a registered model as one the owner may choose (DEC-2136)", () => {
  it("asks each declared parameter from its bounds, and keeps the registered version and hash", () => {
    const choice = servedModelChoice(SERVED)!;
    expect(choice).toMatchObject({ id: "quant.ma_crossover", name: "Moving-average crossover", what: "How it decides.", version: "1.0.0", contentHash: MOCK_MODEL.content_hash });
    expect(choice.params.map((p) => [p.key, p.question])).toEqual([
      ["fast_periods", "What fast periods should it use? A whole number from 1 to 1000."],
      ["slow_periods", "What slow periods should it use? A whole number from 2 to 1000."],
    ]);
    expect(choice.params[0].read("0")).toMatchObject({ ok: false });
    expect(choice.params[0].read("1001")).toMatchObject({ ok: false });
    expect(choice.params[0].read(" 7 ")).toEqual({ ok: true, value: "7" });
  });

  it("offers no model it cannot hold the owner to: one admitting instruments, without content, with an unbounded, non-integer or undeclared parameter, or with a constraint it cannot read", () => {
    const content = SERVED.content;
    for (const m of [
      { ...SERVED, admits_instruments: true },
      { ...SERVED, content: null },
      { ...SERVED, content: { ...content, params: [{ ...content.params[0], max: null }, content.params[1]] } },
      { ...SERVED, content: { ...content, params: [{ ...content.params[0], type: "decimal" }, content.params[1]] } },
      { ...SERVED, content: { ...content, params: [content.params[1]] } },
      { ...SERVED, content: { ...content, constraints: ["fast_periods <= slow_periods"] } },
      { ...SERVED, content: { ...content, constraints: ["fast_periods + slow_periods < 100"] } },
      { ...SERVED, content: { ...content, constraints: ["fast_periods < other_periods"] } },
    ])
      expect(servedModelChoice(m)).toBeNull();
  });

  it("holds the stated a < b constraint once both are given, and only then", () => {
    const choice = servedModelChoice(SERVED)!;
    expect(constraintBroken(choice, { fast_periods: "20" })).toBeNull();
    expect(constraintBroken(choice, { fast_periods: "20", slow_periods: "20" })).toBe("Fast periods must be less than slow periods.");
    expect(constraintBroken(choice, { fast_periods: "5", slow_periods: "20" })).toBeNull();
    expect(readStrategy({ model: choice.id, params: { fast_periods: "30", slow_periods: "20" } }, [choice])).toEqual({ ok: false, error: "Fast periods must be less than slow periods." });
    expect(readStrategy({ model: choice.id, params: { fast_periods: "5", slow_periods: "20" } }, [choice])).toMatchObject({ ok: true, value: { params: [{ key: "fast_periods", value: "5" }, { key: "slow_periods", value: "20" }] } });
  });
});

describe("the catalogue reads, as the set-up decodes them", () => {
  const instrument = { instrument_id: "eb82ffb0-efd6-4f4e-9307-8fead28e04f6", symbol: "QQQ", asset_class: "us_equity", snapshot: `sha256:${"12".repeat(32)}`, registered_at: "2026-10-26T14:00:00.000000000Z" };
  it("reads instruments, and refuses an id that is not an asset id or an unknown class", () => {
    expect(instrumentsRead({ instruments: [instrument] }, "")).toEqual({ ok: true, value: { instruments: [instrument] } });
    expect(instrumentsRead({ instruments: [{ ...instrument, instrument_id: "QQQ" }] }, "").ok).toBe(false);
    expect(instrumentsRead({ instruments: [{ ...instrument, asset_class: "option" }] }, "").ok).toBe(false);
  });

  it("reads a model's content projection, and makes one model's unreadable parameters that model's alone", () => {
    const read = signalModelsRead({ models: [MOCK_MODEL] }, "");
    expect(read.ok && read.value.models[0].content).toEqual({ methodology: MOCK_MODEL.content.methodology, params: [{ name: "fast_periods", type: "integer", min: 1, max: 1000 }, { name: "slow_periods", type: "integer", min: 2, max: 1000 }], constraints: ["fast_periods < slow_periods"] });
    const broken = signalModelsRead({ models: [{ ...MOCK_MODEL, content: { params_schema: { params: [{ name: 7 }] } } }, MOCK_MODEL] }, "");
    expect(broken.ok && broken.value.models.map((m) => m.content === null)).toEqual([true, false]);
  });
});
