import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { drawn, spoken } from "@/test/spoken";
import { HeroFigure } from "./money";

describe("the hero figure", () => {
  it.each([
    ["$28,478.36", "$28,478", ".36"],
    ["$1,234,567.89", "$1,234,567", ".89"],
    ["\u2212$1,234.56", "\u2212$1,234", ".56"],
    ["$0.00", "$0", ".00"],
  ])("%s: draws the cents apart, half size, muted and raised", (value, whole, cents) => {
    for (const instant of [false, true]) {
      const { container, unmount } = render(<HeroFigure value={value} instant={instant} />);
      const part = container.querySelector("[data-slot=cents]")!;
      expect(part.textContent).toBe(cents);
      expect(part.className).toMatch(/\btext-\[0\.5em\]/);
      expect(part.className).toMatch(/\btext-muted-foreground\b/);
      expect(part.className).toMatch(/\balign-\[1cap\]/);
      expect(part.className).toMatch(/\bleading-0\b/);
      expect(drawn(container)).toBe(whole + cents);
      unmount();
    }
  });

  it.each([false, true])("instant=%s: reads the whole value once, never in pieces", (instant) => {
    const { container } = render(<HeroFigure value="$28,478.36" instant={instant} />);
    expect(spoken(container)).toBe("$28,478.36");
    const copy = container.querySelector(".sr-only")!;
    expect(copy.textContent).toBe("$28,478.36");
    expect(copy.children).toHaveLength(0);
    expect(container.querySelector("[data-slot=cents]")!.closest("[aria-hidden=true]")).not.toBeNull();
    expect(container.querySelector("[aria-live]")).toBeNull();
  });

  it("rolls a live change and swaps a scrubbed one in place, drawn in parts either way", () => {
    const { container, rerender } = render(<HeroFigure value="$28,478.36" />);
    expect(container.querySelector("[data-instant]")).toBeNull();
    expect(container.querySelector("[style*=translateY]")).not.toBeNull();
    rerender(<HeroFigure value="$28,460.74" instant />);
    expect(container.querySelector("[data-instant]")).not.toBeNull();
    expect(container.querySelector("[style*=translateY]")).toBeNull();
    expect(spoken(container)).toBe("$28,460.74");
    expect(container.querySelector("[data-slot=cents]")).toHaveTextContent(".74");
  });

  it("draws a value with no cents whole", () => {
    const { container } = render(<HeroFigure value="—" />);
    expect(container.querySelector("[data-slot=cents]")).toBeNull();
    expect(spoken(container)).toBe("—");
  });
});
