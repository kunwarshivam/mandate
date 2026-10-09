import { describe, expect, it } from "vitest";
import { array, closedEnum, contentRef, decimal, integer, nullable, object, string, timestamp, watermark } from "./decode";
import type { Decoded, DecodeIssue } from "./types";

function issue<T>(result: Decoded<T>): DecodeIssue {
  if (result.ok) throw new Error(`expected a decode issue, got ${JSON.stringify(result.value)}`);
  return result.issue;
}

describe("decimal members (spec §3.1, journal spec §4.6)", () => {
  it("pending E11-9: keeps a canonical decimal string exactly as sent", () => {
    for (const text of ["0", "-1.5", "123.000001", "78999999999999999999999999999.9999999999999999999999999999", "0.0000000000000000000000000001"]) {
      expect(decimal(text, "/v")).toEqual({ ok: true, value: text });
    }
  });

  it("pending E11-9: refuses a JSON number in a decimal member, never rounding it", () => {
    for (const number of [1.5, 1, 0, -2, 1e21, 0.1 + 0.2]) {
      expect(issue(decimal(number, "/equity"))).toMatchObject({ path: "/equity", problem: "decimal_number" });
    }
  });

  it("pending E11-9: refuses a string that is not in canonical form", () => {
    const bad = ["1.50", "+1", "-0", "1e3", " 1", "1 ", "NaN", "Infinity", ".5", "5.", "01", "1_000", "", "0.00000000000000000000000000001", "79000000000000000000000000000", "-79000000000000000000000000000"];
    for (const text of bad) {
      expect(issue(decimal(text, "/v")), text).toMatchObject({ path: "/v", problem: "not_canonical", value: text });
    }
  });

  it("pending E11-9: calls anything else in a decimal member the wrong type", () => {
    for (const value of [null, true, {}, ["1"]]) {
      expect(issue(decimal(value, "/v")).problem).toBe("wrong_type");
    }
  });
});

describe("timestamps, refs and integers", () => {
  it("pending E11-9: accepts only journal spec §4.7 timestamps", () => {
    expect(timestamp("2026-09-28T18:05:20.123456789Z", "/t")).toEqual({ ok: true, value: "2026-09-28T18:05:20.123456789Z" });
    for (const text of ["2026-09-28T14:05:20-04:00", "2026-09-28T18:05:20.123Z", "2026-09-28T18:05:20Z", "2026-09-28 18:05:20.123456789Z"]) {
      expect(issue(timestamp(text, "/t")), text).toMatchObject({ problem: "not_canonical" });
    }
    expect(issue(timestamp(1759082720, "/t")).problem).toBe("wrong_type");
  });

  it("pending E11-9: refuses timestamps outside journal spec §4.7's calendar range", () => {
    for (const text of ["1970-01-01T00:00:00.000000000Z", "9999-12-31T23:59:59.999999999Z", "2028-02-29T12:00:00.000000000Z"]) {
      expect(timestamp(text, "/t"), text).toEqual({ ok: true, value: text });
    }
    const bad = [
      "1969-12-31T23:59:59.999999999Z",
      "2026-09-28T18:05:60.000000000Z",
      "2026-09-28T24:00:00.000000000Z",
      "2026-09-28T18:60:00.000000000Z",
      "2026-13-01T00:00:00.000000000Z",
      "2026-00-10T00:00:00.000000000Z",
      "2026-09-00T00:00:00.000000000Z",
      "2026-02-29T00:00:00.000000000Z",
      "2026-04-31T00:00:00.000000000Z",
      "2100-02-29T00:00:00.000000000Z",
    ];
    for (const text of bad) {
      expect(issue(timestamp(text, "/t")), text).toMatchObject({ problem: "not_canonical", value: text });
    }
  });

  it("pending E11-9: accepts only sha256 content refs", () => {
    const ref = `sha256:${"ab".repeat(32)}`;
    expect(contentRef(ref, "/h")).toEqual({ ok: true, value: ref });
    for (const text of ["ab".repeat(32), `sha256:${"AB".repeat(32)}`, `sha256:${"ab".repeat(31)}`]) {
      expect(issue(contentRef(text, "/h")).problem).toBe("not_canonical");
    }
  });

  it("pending E11-9: reads integers from 0 to 2^53 - 1 only", () => {
    expect(integer(0, "/n")).toEqual({ ok: true, value: 0 });
    expect(integer(2 ** 53 - 1, "/n")).toEqual({ ok: true, value: 2 ** 53 - 1 });
    for (const value of [1.5, -1, 2 ** 53, "3"]) {
      expect(issue(integer(value, "/n")).problem).toBe("wrong_type");
    }
  });
});

describe("closed safety enums (spec §3.2)", () => {
  const effect = closedEnum(["none", "recorded", "unknown"]);

  it("pending E11-9: reads a listed value", () => {
    expect(effect("recorded", "/effect")).toEqual({ ok: true, value: "recorded" });
  });

  it("pending E11-9: makes an unlisted value a typed issue naming the value and the allowed set", () => {
    expect(issue(effect("probably", "/effect"))).toEqual({
      path: "/effect",
      problem: "unknown_enum_value",
      value: "probably",
      allowed: ["none", "recorded", "unknown"],
    });
  });

  it("pending E11-9: calls a non-string the wrong type", () => {
    expect(issue(effect(1, "/effect")).problem).toBe("wrong_type");
  });
});

describe("objects and arrays", () => {
  const point = object<{ name: string; size: string | null; tags: string[] }>({
    name: string,
    size: nullable(decimal),
    tags: array(string),
  });

  it("pending E11-9: ignores members it does not know (spec §3.2)", () => {
    expect(point({ name: "a", size: "1.5", tags: ["x"], added_in_v1_1: { anything: 1 } }, "")).toEqual({
      ok: true,
      value: { name: "a", size: "1.5", tags: ["x"] },
    });
  });

  it("pending E11-9: reports a missing member at its pointer", () => {
    expect(issue(point({ name: "a", tags: [] }, "/agents/0"))).toMatchObject({ path: "/agents/0/size", problem: "missing" });
  });

  it("pending E11-9: reports an issue inside an array at its index", () => {
    expect(issue(point({ name: "a", size: 2, tags: [] }, ""))).toMatchObject({ path: "/size", problem: "decimal_number" });
    expect(issue(point({ name: "a", size: null, tags: ["x", 3] }, ""))).toMatchObject({ path: "/tags/1", problem: "wrong_type" });
  });

  it("pending E11-9: refuses a non-object body", () => {
    for (const value of [null, [], "x", 1]) {
      expect(issue(point(value, "")).problem).toBe("wrong_type");
    }
  });
});

describe("watermarks (spec §6.1)", () => {
  it("pending E11-9: reads a stream's seq, hash and recorded_at", () => {
    const mark = { stream_id: "control", seq: 42, hash: `sha256:${"0f".repeat(32)}`, recorded_at: "2026-09-28T18:05:20.000000000Z" };
    expect(watermark({ ...mark, extra: true }, "/as_of/0")).toEqual({ ok: true, value: mark });
    expect(issue(watermark({ ...mark, seq: "42" }, "/as_of/0"))).toMatchObject({ path: "/as_of/0/seq" });
  });
});
