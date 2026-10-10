import { useRef } from "react";
import { act, render } from "@testing-library/react";
import { MotionConfig, type MotionValue } from "motion/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentMode } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { AgentOwl, FEATHERS, HatchingOwl, Owl, type OwlMind, OwnMinds, beakFor, mindOf, moodFor, owlRows, owlShape, useGaze } from "./owl";

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

describe("owls with minds of their own (DEC-906)", () => {
  const rhythm = (svg: Element) => `${(svg as SVGElement).style.getPropertyValue("--owl-blink")} ${(svg as SVGElement).style.getPropertyValue("--owl-blink-offset")}`;
  const flock = (n: number) => Array.from({ length: n }, (_, i) => <Owl key={i} seed="owlhead" mood="awake" />);

  it("gives every owl of one seed its own blink inside, and the seed's one rhythm outside", () => {
    const own = render(<OwnMinds>{flock(6)}</OwnMinds>).container.querySelectorAll("svg[data-slot=owl]");
    expect(new Set([...own].map(rhythm)).size, "six logo owls, six rhythms").toBe(6);
    const { every, offset } = owlShape("owlhead").blink;
    const plain = render(<>{flock(3)}</>).container.querySelectorAll("svg[data-slot=owl]");
    for (const svg of plain) expect(rhythm(svg)).toBe(`${every}s -${offset}s`);
  });

  it("draws the same minds on the server and in the browser, so the page hydrates as it was sent", () => {
    const sent = renderToStaticMarkup(<OwnMinds>{flock(4)}</OwnMinds>);
    expect(renderToStaticMarkup(<OwnMinds>{flock(4)}</OwnMinds>)).toBe(sent);
  });

  it("keeps each temperament in bounds: a blink every few seconds, a spring that settles, a pause under half a second, a glance every few seconds", () => {
    const minds = Array.from({ length: 300 }, (_, i) => mindOf(`owlhead«r${i}»`));
    expect(mindOf("owlhead«r1»")).toEqual(mindOf("owlhead«r1»"));
    for (const m of minds) {
      expect(m.blink.every).toBeGreaterThanOrEqual(3.2);
      expect(m.blink.every).toBeLessThanOrEqual(7.6);
      expect(m.blink.offset).toBeGreaterThanOrEqual(0);
      expect(m.blink.offset).toBeLessThan(m.blink.every);
      expect(m.spring.stiffness).toBeGreaterThanOrEqual(110);
      expect(m.spring.stiffness).toBeLessThanOrEqual(340);
      expect(m.spring.damping).toBeGreaterThanOrEqual(14);
      expect(m.lag).toBeGreaterThanOrEqual(0);
      expect(m.lag).toBeLessThanOrEqual(420);
      expect(m.rest).toBeGreaterThanOrEqual(2400);
      expect(m.rest).toBeLessThanOrEqual(8800);
    }
    expect(new Set(minds.map((m) => m.blink.every)).size).toBeGreaterThan(20);
    expect(new Set(minds.map((m) => m.lag)).size).toBeGreaterThan(50);
  });

  describe("the gaze", () => {
    afterEach(() => vi.useRealTimers());

    function watch(mind: OwlMind) {
      const seen: { gaze?: [MotionValue<number>, MotionValue<number>] } = {};
      function Eyes() {
        const ref = useRef<SVGSVGElement>(null);
        seen.gaze = useGaze(ref, { reach: 4, looking: true, mind });
        return <svg ref={ref} />;
      }
      render(<Eyes />);
      const svg = document.querySelector("svg")!;
      svg.getBoundingClientRect = () => ({ left: 100, top: 100, width: 40, height: 40, right: 140, bottom: 140, x: 100, y: 100, toJSON: () => ({}) });
      const [x, y] = seen.gaze!;
      const setX = vi.spyOn(x, "set");
      const setY = vi.spyOn(y, "set");
      return { setX, setY };
    }

    const MIND: OwlMind = { blink: { every: 4, offset: 1 }, spring: { stiffness: 200, damping: 20, mass: 0.6 }, lag: 300, rest: 2000 };

    it("turns to the pointer only after its own pause", () => {
      vi.useFakeTimers();
      const { setX } = watch(MIND);
      act(() => {
        window.dispatchEvent(new PointerEvent("pointermove", { clientX: 900, clientY: 120 }));
        vi.advanceTimersByTime(20);
      });
      expect(setX, "not yet: it has not noticed").not.toHaveBeenCalled();
      act(() => vi.advanceTimersByTime(MIND.lag));
      expect(setX).toHaveBeenLastCalledWith(4);
    });

    it("glances somewhere of its own now and then, and comes back to the pointer", () => {
      vi.useFakeTimers();
      vi.spyOn(Math, "random").mockReturnValue(0.5);
      const { setX, setY } = watch({ ...MIND, lag: 0 });
      act(() => {
        window.dispatchEvent(new PointerEvent("pointermove", { clientX: 900, clientY: 120 }));
        vi.advanceTimersByTime(20);
      });
      expect(setX).toHaveBeenLastCalledWith(4);
      act(() => vi.advanceTimersByTime(MIND.rest));
      expect(setX, "half a turn: it looks the other way").toHaveBeenLastCalledWith(-4);
      expect(setY.mock.lastCall?.[0]).toBeCloseTo(0);
      act(() => vi.advanceTimersByTime(1100));
      expect(setX, "and back to the pointer").toHaveBeenLastCalledWith(4);
      vi.mocked(Math.random).mockRestore();
    });
  });
});
