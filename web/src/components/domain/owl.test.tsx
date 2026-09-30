import { render } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AgentMode } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { AgentOwl, Owl, moodFor, owlShape } from "./owl";

const MODES: AgentMode[] = ["normal", "exits_only", "paused", "stopped"];

describe("an agent's owl", () => {
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
