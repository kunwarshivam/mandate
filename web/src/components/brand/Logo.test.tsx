import { readFileSync } from "node:fs";
import path from "node:path";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LOCKUP_VIEWBOX, MARK_PATH, MARK_VIEWBOX, OwlheadLockup, OwlheadMark, OwlheadWordmark, WORDMARK_PATH, WORDMARK_VIEWBOX } from "./Logo";

const BRAND = path.resolve(__dirname);

function source(name: string) {
  const svg = readFileSync(path.join(BRAND, name), "utf8");
  return {
    d: /\sd="([^"]+)"/.exec(svg)?.[1],
    viewBox: /viewBox="([^"]+)"/.exec(svg)?.[1],
  };
}

const box = (b: { x: number; y: number; width: number; height: number }) => `${b.x} ${b.y} ${b.width} ${b.height}`;

describe("Owlhead logo", () => {
  it("inlines exactly the paths and view boxes of the committed SVG sources", () => {
    const mark = source("owlhead-mark.svg");
    const wordmark = source("owlhead-wordmark.svg");
    expect(MARK_PATH).toBe(mark.d);
    expect(box(MARK_VIEWBOX)).toBe(mark.viewBox);
    expect(WORDMARK_PATH).toBe(wordmark.d);
    expect(box(WORDMARK_VIEWBOX)).toBe(wordmark.viewBox);
  });

  it.each([
    ["mark", OwlheadMark],
    ["wordmark", OwlheadWordmark],
    ["lockup", OwlheadLockup],
  ])("names the %s Owlhead by default and fills it with currentColor", (_, Logo) => {
    const { container } = render(<Logo className="h-8" />);
    const svg = screen.getByRole("img", { name: "Owlhead" });
    expect(svg).toHaveClass("h-8");
    expect(svg).not.toHaveAttribute("aria-hidden");
    for (const p of container.querySelectorAll("path")) expect(p).toHaveAttribute("fill", "currentColor");
  });

  it.each([
    ["mark", OwlheadMark],
    ["wordmark", OwlheadWordmark],
    ["lockup", OwlheadLockup],
  ])("hides the %s from assistive technology when it is decorative", (_, Logo) => {
    const { container } = render(<Logo title="" />);
    expect(screen.queryByRole("img")).toBeNull();
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("takes a different accessible name when given one", () => {
    render(<OwlheadMark title="Owlhead home" />);
    expect(screen.getByRole("img", { name: "Owlhead home" })).toBeInTheDocument();
  });

  it("sizes the lockup from the mark's height: the mark is the full height and the wordmark sits inside it", () => {
    const { container } = render(<OwlheadLockup />);
    const svg = container.querySelector("svg");
    expect(svg).toHaveAttribute("viewBox", box(LOCKUP_VIEWBOX));
    expect(LOCKUP_VIEWBOX.height).toBe(MARK_VIEWBOX.height);
    const transform = container.querySelectorAll("path")[1].getAttribute("transform") ?? "";
    const [tx, ty, scale] = /translate\(([\d.]+) ([\d.]+)\) scale\(([\d.]+)\)/.exec(transform)?.slice(1).map(Number) ?? [];
    expect(tx).toBeGreaterThan(MARK_VIEWBOX.width);
    expect(ty + WORDMARK_VIEWBOX.height * scale).toBeLessThanOrEqual(MARK_VIEWBOX.height);
    expect(tx + WORDMARK_VIEWBOX.width * scale).toBeCloseTo(LOCKUP_VIEWBOX.width, 6);
  });

  it("makes no request: no external href, image, or font", () => {
    const { container } = render(
      <>
        <OwlheadMark />
        <OwlheadWordmark />
        <OwlheadLockup />
      </>,
    );
    expect(container.querySelector("image, use, text, foreignObject, [href], [xlink\\:href]")).toBeNull();
  });
});
