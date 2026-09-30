import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { drawn, spoken } from "@/test/spoken";
import { HeroFigure, Money, SignedMoney, parseUsd } from "./money";

const CSS = readFileSync(join(__dirname, "../../app/globals.css"), "utf8");

describe("the hero figure", () => {
  it.each([
    ["$28,478.36", "$28,478", ".36"],
    ["$1,234,567.89", "$1,234,567", ".89"],
    ["\u2212$1,234.56", "\u2212$1,234", ".56"],
    ["$0.00", "$0", ".00"],
  ])("%s: draws the cents apart, as Number Flow's fraction part", (value, whole, cents) => {
    for (const instant of [false, true]) {
      const { container, unmount } = render(<HeroFigure value={value} instant={instant} />);
      expect(container.querySelector("[data-part=fraction]")!.textContent).toBe(cents);
      expect(container.querySelector("[data-number-flow]")).toHaveClass("number-flow-cents");
      expect(drawn(container)).toBe(whole + cents);
      unmount();
    }
  });

  it("styles the cents half size, muted and raised to the cap height, and the roll's edges flat", () => {
    const cents = /\.number-flow-cents::part\(fraction\)\s*\{([^}]*)\}/.exec(CSS)?.[1] ?? "";
    expect(cents).toMatch(/font-size:\s*0\.5em/);
    expect(cents).toMatch(/color:\s*var\(--muted-foreground\)/);
    expect(cents).toMatch(/vertical-align:\s*1cap/);
    const flow = /\.number-flow\s*\{([^}]*)\}/.exec(CSS)?.[1] ?? "";
    expect(flow).toMatch(/--number-flow-mask-height:\s*0px/);
    expect(flow).toMatch(/--number-flow-mask-width:\s*0px/);
  });

  it.each([false, true])("instant=%s: reads the whole value once, never in pieces", (instant) => {
    const { container } = render(<HeroFigure value="$28,478.36" instant={instant} />);
    expect(spoken(container)).toBe("$28,478.36");
    const copy = container.querySelector(".sr-only")!;
    expect(copy.textContent).toBe("$28,478.36");
    expect(copy.children).toHaveLength(0);
    expect(container.querySelector("[data-number-flow]")).toHaveAttribute("aria-hidden", "true");
    expect(container.querySelector("[aria-live]")).toBeNull();
  });

  it("rolls a live change and sets a scrubbed one at once", () => {
    const { container, rerender } = render(<HeroFigure value="$28,478.36" />);
    expect(container.querySelector("[data-instant]")).toBeNull();
    expect(container.querySelector("[data-number-flow]")).toHaveAttribute("data-animated");
    rerender(<HeroFigure value="$28,460.74" instant />);
    expect(container.querySelector("[data-instant]")).not.toBeNull();
    expect(container.querySelector("[data-number-flow]")).not.toHaveAttribute("data-animated");
    expect(spoken(container)).toBe("$28,460.74");
    expect(container.querySelector("[data-part=fraction]")).toHaveTextContent(".74");
  });

  it("draws a value that is not a dollar figure whole, with the plain roll", () => {
    const { container } = render(<HeroFigure value="—" />);
    expect(container.querySelector("[data-number-flow]")).toBeNull();
    expect(container.querySelector("[style*=translateY]")).not.toBeNull();
    expect(spoken(container)).toBe("—");
  });
});

describe("figures", () => {
  it.each([
    ["$1,234.50", { sign: "", amount: 1234.5, places: 2 }],
    ["+$55.56", { sign: "+", amount: 55.56, places: 2 }],
    ["\u2212$67.89", { sign: "\u2212", amount: 67.89, places: 2 }],
    ["-$5", { sign: "\u2212", amount: 5, places: 0 }],
    ["$0.1234", { sign: "", amount: 0.1234, places: 4 }],
  ])("reads %s as its sign, amount and places", (value, parts) => {
    expect(parseUsd(value)).toEqual(parts);
  });

  it.each(["—", "12 XYZ", "$1,23.00", "USD 5"])("leaves %s to the plain roll", (value) => {
    expect(parseUsd(value)).toBeNull();
  });

  it("draws a signed change with its true minus sign, and money at the places asked for", () => {
    const loss = render(<SignedMoney value="-67.89" />);
    expect(drawn(loss.container)).toBe("\u2212$67.89loss");
    loss.unmount();
    const money = render(<Money value="0.1234" places={4} />);
    expect(drawn(money.container)).toBe("$0.1234");
  });
});
