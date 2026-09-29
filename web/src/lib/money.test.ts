import { describe, expect, it } from "vitest";
import { add, dec, div, mul, ratio, toDecimalString, toFixed } from "./decimal";
import { MINUS, age, ago, clock, direction, directionWord, percent, price, quantity, remaining, signedUsd, usd } from "./format";

describe("decimal", () => {
  it("adds without binary floating-point error", () => {
    expect(toDecimalString(add(dec("0.1"), dec("0.2")))).toBe("0.3");
  });

  it("multiplies and divides at 12 places", () => {
    expect(toDecimalString(mul(dec("10000"), dec("0.02")))).toBe("200");
    expect(toDecimalString(div(dec("1"), dec("3")))).toBe("0.333333333333");
  });

  it("rounds half away from zero and never prints negative zero", () => {
    expect(toFixed(dec("1.005"), 2)).toBe("1.01");
    expect(toFixed(dec("-1.005"), 2)).toBe("-1.01");
    expect(toFixed(dec("-0.001"), 2)).toBe("0.00");
  });

  it("rejects strings that are not schema decimals", () => {
    expect(() => dec("1e3")).toThrow();
    expect(() => dec("01")).toThrow();
    expect(() => dec("0.1234567890123")).toThrow();
  });

  it("clamps drawing ratios to [0, 1]", () => {
    expect(ratio(dec("150"), dec("100"))).toBe(1);
    expect(ratio(dec("-5"), dec("100"))).toBe(0);
    expect(ratio(dec("25"), dec("100"))).toBe(0.25);
    expect(ratio(dec("1"), dec("0"))).toBe(0);
  });
});

describe("format", () => {
  it("writes dollars with grouping and a true minus sign", () => {
    expect(usd("1234.5")).toBe("$1,234.50");
    expect(usd("-67.891")).toBe(`${MINUS}$67.89`);
    expect(usd("10000", 0)).toBe("$10,000");
  });

  it("signs gains and losses in text, not only in colour", () => {
    expect(signedUsd("123.45")).toBe("+$123.45");
    expect(signedUsd("-67.89")).toBe(`${MINUS}$67.89`);
    expect(signedUsd("0")).toBe("$0.00");
    expect([direction("5"), direction("-5"), direction("0")]).toEqual(["gain", "loss", "flat"]);
    expect([directionWord("5"), directionWord("-5"), directionWord("0")]).toEqual(["gain", "loss", "no change"]);
  });

  it("keeps an instrument's price precision", () => {
    expect(price("56700")).toBe("$56,700.00");
    expect(price("97.625")).toBe("$97.625");
    expect(quantity("0.015")).toBe("0.015");
    expect(percent("0.075")).toBe("7.5%");
    expect(percent("0.08", 0)).toBe("8%");
  });

  it("reads wall-clock time from the ISO string, independent of the viewer's zone", () => {
    expect(clock("2026-09-28T14:02:11-04:00")).toBe("14:02:11");
  });

  it("states ages and remaining time in whole units", () => {
    expect(ago("2026-09-28T14:02:11-04:00", "2026-09-28T14:05:20-04:00")).toBe("3 min ago");
    expect(remaining("2026-09-28T14:20:00-04:00", "2026-09-28T14:05:20-04:00")).toBe("14 min left");
    expect(remaining("2026-09-28T14:05:40-04:00", "2026-09-28T14:05:20-04:00")).toBe("less than 1 min left");
    expect(remaining("2026-09-28T14:05:00-04:00", "2026-09-28T14:05:20-04:00")).toBe("deadline passed");
  });

  it("states a bare age in steps of five seconds, and an age ago in the same words", () => {
    const now = "2026-09-28T14:05:20-04:00";
    const cases: Array<[string, string]> = [
      ["2026-09-28T14:05:18-04:00", "under 5 s"],
      ["2026-09-28T14:05:10-04:00", "10 s"],
      ["2026-09-28T14:04:21-04:00", "55 s"],
      ["2026-09-28T14:02:11-04:00", "3 min"],
      ["2026-09-28T11:05:20-04:00", "3 h"],
      ["2026-09-28T14:05:30-04:00", "under 5 s"],
    ];
    for (const [from, text] of cases) {
      expect(age(from, now)).toBe(text);
      expect(ago(from, now)).toBe(`${text} ago`);
    }
  });
});
