import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { toHex } from "@/lib/color";
import { INK, NIGHT, OFF_WHITE, hexContrast } from "@/lib/brand-palette";
import { PALETTE, PALETTE_DARK } from "@/lib/palette";
import { BrandSpecimen } from "./brand-specimen";

const rgb = (hex: string) => `rgb(${[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(", ")})`;

describe("/design brand specimen", () => {
  it("shows the mark, wordmark, and lockup in ink on off-white and in off-white on night, and on no other fill", () => {
    const { container } = render(<BrandSpecimen />);
    const surfaces = container.querySelectorAll<HTMLElement>("[data-slot=brand-surface]");
    expect([...surfaces].map((s) => [s.style.background, s.style.color])).toEqual([
      [rgb(OFF_WHITE), rgb(INK)],
      [rgb(NIGHT), rgb(OFF_WHITE)],
    ]);
    for (const surface of surfaces) expect(within(surface).getAllByRole("img", { name: "Owlhead" })).toHaveLength(3);
    const fills = [...container.querySelectorAll<HTMLElement>("[style]")].filter((el) => el.style.background && !el.closest("[data-slot=brand-color]") && el.dataset.slot !== "brand-surface");
    expect(fills).toEqual([]);
    for (const mark of container.querySelectorAll("[data-slot=owlhead-mark], [data-slot=owlhead-lockup]")) {
      if (mark.closest("[data-slot=brand-surface]")) continue;
      expect(mark.closest<HTMLElement>("[style]")?.style.color).toBe("var(--logo)");
    }
  });

  it("lists the six palette values with their contrast", () => {
    render(<BrandSpecimen />);
    const palette = screen.getByRole("list", { name: "Brand palette" });
    const text = within(palette).getAllByRole("listitem").map((li) => li.textContent);
    expect(text).toHaveLength(6);
    for (const hex of ["#14161A", "#FBFDFE", "#7C9217", "#4C5A09", "#F2FCD7", "#0B0D11"]) expect(text.join(" ")).toContain(hex);
    expect(text[0]).toContain(`${hexContrast(INK, OFF_WHITE).toFixed(2)}:1 on off-white`);
    expect(hexContrast(INK, OFF_WHITE)).toBeGreaterThanOrEqual(15);
    expect(hexContrast(OFF_WHITE, NIGHT)).toBeGreaterThanOrEqual(15);
  });

  it("takes its colours from the palette: ink and off-white are light mode's type and card, night is dark mode's page", () => {
    expect(INK).toBe(toHex(PALETTE.tokens.foreground.value).toUpperCase());
    expect(OFF_WHITE).toBe(toHex(PALETTE.tokens.card.value).toUpperCase());
    expect(NIGHT).toBe(toHex(PALETTE_DARK.tokens.background.value).toUpperCase());
  });

  it("shows the minimum sizes: the mark at 16 px, the lockup 96 px wide", () => {
    const { container } = render(<BrandSpecimen />);
    expect(container.querySelector("[data-slot=owlhead-mark].h-4")).not.toBeNull();
    expect(container.querySelector("[data-slot=owlhead-lockup].w-24")).not.toBeNull();
  });
});
