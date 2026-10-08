import { render } from "@testing-library/react";
import { MotionConfig } from "motion/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AgentMode } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { AgentOwl, FEATHERS, HatchingOwl, Owl, beakFor, moodFor, owlRows, owlShape } from "./owl";

const MODES: AgentMode[] = ["normal", "exits_only", "paused", "stopped"];

describe("an agent's owl", () => {
  it("hatches once from an egg on its own grid, and under reduced motion is simply there (DEC-514)", () => {
    const { container, unmount } = render(<HatchingOwl seed={AGENT_IDS.btc} className="size-16" />);
    const hatch = container.querySelector("[data-slot=hatch]")!;
    expect(hatch.querySelector("[data-slot=owl]")).toHaveAttribute("data-mood", "awake");
    expect(hatch.querySelector("[data-slot=egg]")).not.toBeNull();
    expect(hatch.querySelector("[data-slot=egg]")!.querySelectorAll("rect").length).toBeGreaterThan(10);
    unmount();
    const reduced = render(
      <MotionConfig reducedMotion="always">
        <HatchingOwl seed={AGENT_IDS.btc} />
      </MotionConfig>,
    ).container.querySelector("[data-slot=hatch]")!;
    expect(reduced).toHaveAttribute("data-reduced");
    expect(reduced.querySelector("[data-slot=egg]")).toBeNull();
    expect(reduced.querySelector("[data-slot=owl]")).not.toBeNull();
  });

  it("draws the same owl for the same ID, every time", () => {
    for (const id of Object.values(AGENT_IDS)) {
      expect(owlShape(id)).toEqual(owlShape(id));
      expect(renderToStaticMarkup(<Owl seed={id} mood="awake" />)).toBe(renderToStaticMarkup(<Owl seed={id} mood="awake" />));
    }
  });

  it("gives the fixture's agents different faces", () => {
    const faces = Object.values(AGENT_IDS).map((id) => JSON.stringify(owlShape(id)));
    expect(new Set(faces).size).toBe(faces.length);
  });

  it("shows the mode in its eyes: open, half closed on exits only, asleep when paused, shut when stopped", () => {
    expect(MODES.map(moodFor)).toEqual(["awake", "focused", "asleep", "stopped"]);
    const shapes = MODES.map((mode) => renderToStaticMarkup(<AgentOwl agent={{ agent_id: AGENT_IDS.btc, mode }} />));
    expect(new Set(shapes).size).toBe(MODES.length);
  });

  it("reads nothing but its ID and mode, so a gain or a loss never changes its face", () => {
    const [agent] = buildWorkspace("normal").agents;
    const ahead = { ...agent, pnl_today: "500.00", pnl_total: "900.00" };
    const behind = { ...agent, pnl_today: "-500.00", pnl_total: "-900.00" };
    expect(renderToStaticMarkup(<AgentOwl agent={ahead} />)).toBe(renderToStaticMarkup(<AgentOwl agent={behind} />));
  });

  it("moves only as its mode allows: open eyes blink, a sleeping owl breathes, a stopped owl is still", () => {
    const owl = (mode: AgentMode, still?: boolean) => render(<AgentOwl agent={{ agent_id: AGENT_IDS.btc, mode }} still={still} />).container.querySelector("svg")!;
    expect(owl("normal").querySelectorAll(".owl-blink")).toHaveLength(2);
    expect(owl("exits_only").querySelectorAll(".owl-blink")).toHaveLength(2);
    expect(owl("paused").querySelectorAll(".owl-blink")).toHaveLength(0);
    expect(owl("paused").querySelectorAll(".owl-z")).toHaveLength(2);
    expect(owl("stopped").querySelectorAll(".owl-blink, .owl-z")).toHaveLength(0);
    const still = owl("paused", true);
    expect(still).toHaveAttribute("data-still");
    expect(still.querySelectorAll(".owl-z")).toHaveLength(0);
  });

  it("blinks on its own rhythm, so owls on one screen never blink together", () => {
    const rhythms = Object.values(AGENT_IDS).map((id) => JSON.stringify(owlShape(id).blink));
    expect(new Set(rhythms).size).toBeGreaterThan(1);
    for (const id of Object.values(AGENT_IDS)) {
      const { every, offset } = owlShape(id).blink;
      expect(offset).toBeGreaterThanOrEqual(0);
      expect(offset).toBeLessThan(every);
    }
  });

  it("draws every set of ears on one 16 by 16 grid, outlined, with both eyes where the lids and pupils land", () => {
    for (const ears of ["tufts", "wide", "round"] as const) {
      const rows = owlRows(ears);
      expect(rows).toHaveLength(16);
      for (const row of rows) expect(row, ears).toHaveLength(16);
      for (const x of [2, 10]) {
        for (let y = 4; y <= 7; y++) expect(rows[y].slice(x + 1, x + 3), `${ears} eye at ${x},${y}`).toBe("ww");
      }
      for (const row of rows) {
        const ink = row.replace(/\./g, "");
        if (ink.length > 0 && !/^f+$/.test(ink)) expect(row.trim().replace(/^\.+|\.+$/g, "")[0], `${ears} row "${row}" starts on its outline`).toBe("o");
      }
    }
  });

  it("puts belly marks on the belly alone, so they never draw a mouth on the face", () => {
    const rows = owlRows("round");
    for (const id of [...Object.values(AGENT_IDS), "owlhead", "assistant", "a", "b", "c", "d", "e", "f"]) {
      const { container } = render(<Owl seed={id} mood="awake" still />);
      const marks = [...container.querySelectorAll("rect[opacity='0.3']")];
      for (const m of marks) {
        const x = Number(m.getAttribute("x")) / 4;
        const y = Number(m.getAttribute("y")) / 4;
        expect(rows[y][x], `${id} mark at ${x},${y}`).toBe("l");
      }
    }
  });

  it("takes the brand owl's colour in place of its own, and gives a sun owl a dark beak", () => {
    expect(beakFor("var(--series-2)")).toBe("var(--owl-pupil)");
    for (const f of FEATHERS.filter((f) => f !== "var(--series-2)")) expect(beakFor(f)).toBe("var(--highlight)");
    const { container } = render(<Owl seed="owlhead" mood="awake" feathers="var(--brand-owl)" beak="var(--brand-owl-beak)" still />);
    const fills = new Set([...container.querySelectorAll("rect")].map((r) => r.getAttribute("fill")));
    expect(fills).toContain("var(--brand-owl)");
    expect(fills).toContain("var(--brand-owl-beak)");
    for (const f of FEATHERS) expect(fills).not.toContain(f);
  });

  it("is decorative, painted only in palette tokens", () => {
    const { container } = render(<AgentOwl agent={{ agent_id: AGENT_IDS.swing, mode: "paused" }} />);
    const svg = container.querySelector("svg[data-slot=owl]")!;
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("data-mood", "asleep");
    for (const el of svg.querySelectorAll("[fill], [stroke]")) {
      for (const attr of ["fill", "stroke"]) {
        const v = el.getAttribute(attr);
        if (v && v !== "none") expect(v, attr).toMatch(/^var\(--[a-z0-9-]+\)$/);
      }
    }
  });
});
