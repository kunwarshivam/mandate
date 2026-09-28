import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { BrandSpecimen } from "./brand-specimen";

describe("/design brand specimen", () => {
  it("shows the mark, wordmark, and lockup in navy on off-white, and never reversed on navy", () => {
    const { container } = render(<BrandSpecimen />);
    const surfaces = container.querySelectorAll<HTMLElement>("[data-slot=brand-surface]");
    expect([...surfaces].map((s) => [s.style.background, s.style.color])).toEqual([["rgb(247, 250, 254)", "rgb(24, 61, 115)"]]);
    expect(within(surfaces[0]).getAllByRole("img", { name: "Owlhead" })).toHaveLength(3);
    const navyFills = [...container.querySelectorAll<HTMLElement>("[style]")].filter((el) => el.style.background === "rgb(24, 61, 115)" && !el.closest("[data-slot=brand-color]"));
    expect(navyFills).toEqual([]);
  });

  it("lists the six palette values with their contrast", () => {
    render(<BrandSpecimen />);
    const palette = screen.getByRole("list", { name: "Navy and brass palette" });
    const text = within(palette).getAllByRole("listitem").map((li) => li.textContent);
    expect(text).toHaveLength(6);
    for (const hex of ["#183D73", "#AC7D1B", "#634606", "#FDF1DC", "#181C21", "#F7FAFE"]) expect(text.join(" ")).toContain(hex);
    expect(text[0]).toContain("10.27:1 on off-white");
  });

  it("shows the minimum sizes: the mark at 16 px, the lockup 96 px wide", () => {
    const { container } = render(<BrandSpecimen />);
    expect(container.querySelector("[data-slot=owlhead-mark].h-4")).not.toBeNull();
    expect(container.querySelector("[data-slot=owlhead-lockup].w-24")).not.toBeNull();
  });
});
