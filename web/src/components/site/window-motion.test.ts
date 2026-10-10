import { describe, expect, it, vi } from "vitest";
import { leave, leaving, millis, moving, onto, openFrom, reframe } from "./window-motion";

/** Applies `translate(dx, dy) scale(s)` from a top-left origin to a box, as the browser would. */
function apply(box: { left: number; top: number; width: number; height: number }, transform: string) {
  const m = /^translate\((-?[\d.e-]+)px, (-?[\d.e-]+)px\) scale\(([\d.e-]+)\)$/.exec(transform);
  if (!m) throw new Error(`not a zoom: ${transform}`);
  const [dx, dy, s] = m.slice(1).map(Number);
  return { left: box.left + dx, top: box.top + dy, width: box.width * s, height: box.height * s };
}

describe("window motion", () => {
  it("lays a window over its source, centred, keeping its proportions, never stretched", () => {
    const win = { left: 436, top: 187, width: 480, height: 434 };
    for (const from of [
      { left: 12, top: 96, width: 96, height: 84 },
      { left: 287, top: 765, width: 176, height: 32 },
      { left: 500, top: 300, width: 40, height: 400 },
    ]) {
      const got = apply(win, onto(win, from));
      expect(got.width / got.height, "same proportions").toBeCloseTo(win.width / win.height, 6);
      expect(got.left + got.width / 2, "centred across").toBeCloseTo(from.left + from.width / 2, 6);
      expect(got.top + got.height / 2, "centred down").toBeCloseTo(from.top + from.height / 2, 6);
      expect(got.width <= from.width + 1e-9 && got.height <= from.height + 1e-9, "inside the source").toBe(true);
      expect(got.width === from.width || got.height === from.height, "as big as the source allows").toBe(true);
    }
  });

  it("reads a duration token in either unit the stylesheet writes", () => {
    expect(millis("280ms")).toBe(280);
    expect(millis(".28s")).toBe(280);
    expect(millis(" 0.15s ")).toBe(150);
    expect(millis("")).toBeNull();
    expect(millis("fast")).toBeNull();
  });

  it("changes state at once where the browser cannot animate", () => {
    const el = document.createElement("section");
    expect(typeof el.animate, "jsdom has no Web Animations API").not.toBe("function");
    const done = vi.fn();
    leave(el, null, done);
    expect(done).toHaveBeenCalledOnce();
    const change = vi.fn();
    reframe(el, change);
    expect(change).toHaveBeenCalledOnce();
    expect(() => openFrom(el, { left: 0, top: 0, width: 10, height: 10 })).not.toThrow();
    expect(moving(el)).toBe(false);
    expect(leaving(el)).toBe(false);
    expect(moving(null)).toBe(false);
  });
});
