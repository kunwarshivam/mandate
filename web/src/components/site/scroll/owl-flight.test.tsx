import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LongPage } from "./long-page";
import { OwlFlight, nearestPerch, owlSize, perchOn } from "./owl-flight";

/**
 * The flying owl (DEC-907): decoration only, it stands on the perches the page marks, wears tide
 * feathers where its sun ones would vanish, and with motion reduced stands still on the first perch
 * without asking for a single animation frame.
 */

function context() {
  return { setTransform: vi.fn(), clearRect: vi.fn(), save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), rect: vi.fn(), clip: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), closePath: vi.fn(), fill: vi.fn(), stroke: vi.fn(), fillStyle: "", strokeStyle: "", lineWidth: 0, lineJoin: "" };
}

function motion(reduce: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: reduce && query.includes("reduce"), media: query, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
}

function box(el: Element, r: Partial<DOMRect>) {
  vi.spyOn(el, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}), ...r } as DOMRect);
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("the owl's canvas", () => {
  it("is hidden from assistive technology and takes no pointer or focus", () => {
    render(<OwlFlight />);
    const canvas = document.querySelector("canvas")!;
    expect(canvas.dataset.slot).toBe("flying-owl");
    expect(canvas).toHaveAttribute("aria-hidden", "true");
    expect(canvas).not.toHaveAttribute("tabindex");
  });

  it("with motion reduced, stands still on the first perch, drawn once, and asks for no frame", () => {
    motion(true);
    const g = context();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(g as unknown as RenderingContext);
    const frames = vi.spyOn(window, "requestAnimationFrame");
    render(<LongPage />);
    const canvas = document.querySelector<HTMLCanvasElement>("[data-slot=flying-owl]")!;
    expect(canvas.dataset.owl).toBe("resting");
    expect(canvas.style.position).toBe("absolute");
    expect(g.fill).toHaveBeenCalled();
    expect(frames.mock.calls.filter(([cb]) => cb.name === "frame")).toHaveLength(0);
  });

  it("with motion allowed, flies on animation frames and stops when it leaves the page", () => {
    motion(false);
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context() as unknown as RenderingContext);
    const frames = vi.spyOn(window, "requestAnimationFrame");
    const cancel = vi.spyOn(window, "cancelAnimationFrame");
    const { unmount } = render(<LongPage />);
    expect(frames.mock.calls.some(([cb]) => cb.name === "frame")).toBe(true);
    unmount();
    expect(cancel).toHaveBeenCalled();
  });
});

describe("its perches", () => {
  it("stands on an element's top edge, the given way across, with its feet on the line", () => {
    const el = document.createElement("div");
    el.dataset.perchAt = "0.25";
    el.dataset.perchYaw = "0.5";
    box(el, { left: 100, top: 400, width: 800, height: 300, bottom: 700 });
    expect(perchOn(el, 100)).toEqual({ x: 300, y: 400 - 44, yaw: 0.5, coat: "sun" });
    expect(perchOn(el, 100, { left: 50, top: 100 })).toMatchObject({ x: 250, y: 300 - 44 });
  });

  it("picks the perch nearest two fifths down the window, and holds the current one against a near tie", () => {
    const [a, b, gone] = [0, 1, 2].map(() => document.createElement("div"));
    box(a!, { top: 380, bottom: 580, width: 10, height: 200 });
    box(b!, { top: 420, bottom: 600, width: 10, height: 180 });
    box(gone!, { top: 0, bottom: 0, width: 0, height: 0 });
    expect(nearestPerch([a!, b!, gone!], 1000, null)).toBe(b);
    expect(nearestPerch([a!, b!], 1000, a!)).toBe(a);
  });

  it("is smaller on a phone", () => {
    expect(owlSize(390)).toBeLessThan(owlSize(1440));
  });

  it("marks a perch on every picture and part, the first on the opening picture, each between the edges", () => {
    const { container } = render(<LongPage />);
    const perches = [...container.querySelectorAll<HTMLElement>("[data-perch]")];
    expect(perches[0]!.closest("[data-slot=intro]")).not.toBeNull();
    expect(perches.length).toBeGreaterThanOrEqual(8);
    for (const p of perches) {
      const at = Number(p.dataset.perchAt);
      expect(at).toBeGreaterThanOrEqual(0);
      expect(at).toBeLessThanOrEqual(1);
    }
  });

  it("wears tide feathers exactly where it stands on sun, so it never vanishes into its own colour", () => {
    const { container } = render(<LongPage />);
    for (const p of container.querySelectorAll<HTMLElement>("[data-perch]")) {
      const onSun = p.closest(".bg-highlight") !== null;
      expect(p.dataset.perchCoat, p.outerHTML.slice(0, 80)).toBe(onSun ? "tide" : "sun");
    }
  });
});
