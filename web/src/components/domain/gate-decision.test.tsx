import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { buildWorkspace } from "@/fixtures/workspace";
import { measure } from "@/lib/contrast";
import { type PairKind, PAIRS, REQUIREMENT } from "@/lib/contrast-pairs";
import { PALETTES, TOKEN_NAMES, type ThemeName, type TokenName } from "@/lib/palette";
import { VerdictChip } from "./gate-decision";

const THEMES: ThemeName[] = ["light", "dark"];
const allowed = buildWorkspace("normal").decisions.find((d) => d.verdict === "allow" && !d.approval_id);

/** The colour token a utility class names, as `bg-card` names `card`; null for a size or a modifier. */
function tokenOf(classes: string[], prefix: "bg" | "text" | "ring"): TokenName | null {
  const names = classes.flatMap((c) => (c.startsWith(`${prefix}-`) ? [c.slice(prefix.length + 1)] : [])).filter((n): n is TokenName => (TOKEN_NAMES as readonly string[]).includes(n));
  expect(names.length, `at most one ${prefix} colour`).toBeLessThanOrEqual(1);
  return names[0] ?? null;
}

/**
 * Critique C-18: in dark mode the Allowed chip's fill sits a step from the card it rests on, so the
 * chip vanished beside three ringed ones. It keeps an edge in both themes, and every colour it draws
 * is a contrast pair the palette is checked on: its label on its fill, and its edge against its own
 * fill and against what surrounds it (the card field, and the page when a linked row is hovered).
 */
describe("the Allowed chip keeps its edge (C-18)", () => {
  it("is drawn for an allow that went out, as the word Allowed", () => {
    expect(allowed, "the normal fixture has an allow that went out").toBeDefined();
    const { container } = render(<VerdictChip decision={allowed!} />);
    expect(container.querySelector("[data-slot=verdict]")).toHaveTextContent("Allowed");
  });

  const chip = () => {
    const { container } = render(<VerdictChip decision={allowed!} />);
    const classes = container.querySelector("[data-slot=verdict]")!.className.split(/\s+/);
    return { classes, fill: tokenOf(classes, "bg"), text: tokenOf(classes, "text"), edge: tokenOf(classes, "ring") };
  };

  it("draws an inset hairline edge in a palette colour, not only a fill", () => {
    const { classes, fill, text, edge } = chip();
    expect(fill, "a fill token").not.toBeNull();
    expect(text, "a label token").not.toBeNull();
    expect(edge, "an edge colour: without one the chip vanishes on a dark card").not.toBeNull();
    expect(classes, "one hairline, inside the chip so its width stays the verdict column's").toEqual(expect.arrayContaining(["ring-1", "ring-inset"]));
  });

  const needed = (): { fg: TokenName; bg: TokenName; kind: PairKind; what: string }[] => {
    const { fill, text, edge } = chip();
    return [
      { fg: text!, bg: fill!, kind: "body", what: "label on the chip's fill" },
      { fg: edge!, bg: fill!, kind: "mark", what: "edge against the chip's own fill" },
      { fg: edge!, bg: "card", kind: "mark", what: "edge against the card field around it" },
      { fg: edge!, bg: "background", kind: "mark", what: "edge against the page, on a hovered row" },
    ];
  };

  it("names each of its colours in the contrast pairs", () => {
    for (const n of needed()) {
      expect(
        PAIRS.some((p) => p.fg === n.fg && p.bg === n.bg && p.kind === n.kind),
        `${n.what}: ${n.fg} on ${n.bg} (${n.kind}) is a contrast pair`,
      ).toBe(true);
    }
  });

  it.each(THEMES)("keeps text at 4.5:1 and its edge at 3:1 in %s", (theme) => {
    const t = PALETTES[theme].tokens;
    for (const n of needed()) {
      const m = measure(t[n.fg].value, t[n.bg].value, n.kind);
      expect(m.ratio, `${theme}: ${n.what}, ${n.fg} on ${n.bg}`).toBeGreaterThanOrEqual(REQUIREMENT[n.kind].wcag);
    }
  });
});
