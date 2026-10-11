import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LongPage } from "./long-page";
import { OwlFlight, nearestPerch, owlSize, perchOn } from "./owl-flight";
import { CUE_MOVED } from "./scroll-cue";

/**
 * The flying owl (DEC-907): decoration only, it stands on the perches the page marks, wears its
 * pale coat on tide where its ink would sink, and with motion reduced stands still on the first
 * perch without asking for a single animation frame.
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
    expect(frames.mock.calls.filter(([cb]) => cb.name === "fly")).toHaveLength(0);
  });

  it("with motion allowed, flies on animation frames and stops when it leaves the page", () => {
    motion(false);
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context() as unknown as RenderingContext);
    const frames = vi.spyOn(window, "requestAnimationFrame");
    const cancel = vi.spyOn(window, "cancelAnimationFrame");
    const { unmount } = render(<LongPage />);
    expect(frames.mock.calls.some(([cb]) => cb.name === "fly")).toBe(true);
    unmount();
    expect(cancel).toHaveBeenCalled();
  });

  it("with motion reduced, nothing that moves the page asks for a frame", () => {
    motion(true);
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context() as unknown as RenderingContext);
    const frames = vi.spyOn(window, "requestAnimationFrame");
    render(<LongPage />);
    for (const type of ["scroll", "resize"]) window.dispatchEvent(new Event(type));
    document.dispatchEvent(new Event(CUE_MOVED));
    expect(frames.mock.calls.filter(([cb]) => cb.name === "fly")).toHaveLength(0);
  });
});

/**
 * A clock the owl's frames, timers and `performance.now` all run on, starting at 0, so the times of
 * its blinks (every 4.7 s, for 130 ms) and glances (every 6.8 s, for 1.5 s) are known.
 */
function clock() {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "requestAnimationFrame", "cancelAnimationFrame", "performance"] });
  motion(false);
  const g = context();
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(g as unknown as RenderingContext);
  const frames = vi.spyOn(window, "requestAnimationFrame");
  render(<LongPage />);
  const canvas = document.querySelector<HTMLCanvasElement>("[data-slot=flying-owl]")!;
  const asked = () => frames.mock.calls.filter(([cb]) => cb.name === "fly").length;
  const until = (at: number) => act(() => vi.advanceTimersByTime(at - performance.now()));
  return { g, canvas, frames, asked, until };
}

describe("when it has nowhere to go", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("stands perched once every spring has come to rest, and asks for no more frames", () => {
    const { canvas, frames, asked, until } = clock();
    until(4000);
    expect(canvas.dataset.owl).toBe("perched");
    frames.mockClear();
    until(4600);
    expect(asked(), "a standing owl asks for no frame").toBe(0);
  });

  it("blinks while it stands with two paints and no frame, and turns its head to glance on its springs, then stands again", () => {
    const { g, canvas, frames, asked, until } = clock();
    until(4600);
    expect(canvas.dataset.owl).toBe("perched");
    frames.mockClear();
    g.clearRect.mockClear();
    until(4700);
    expect(g.clearRect, "its eyes shut at the blink").toHaveBeenCalledTimes(1);
    until(4900);
    expect(g.clearRect, "and open again").toHaveBeenCalledTimes(2);
    expect(asked(), "a blink is no flight").toBe(0);
    until(6900);
    expect(asked(), "a glance turns the head on its springs").toBeGreaterThan(0);
    until(8250);
    expect(canvas.dataset.owl, "it holds the glance standing").toBe("perched");
    frames.mockClear();
    until(8350);
    expect(asked(), "and looks back when the glance is over").toBeGreaterThan(0);
    until(10000);
    expect(canvas.dataset.owl).toBe("perched");
  }, 20_000);

  it("flies again when the page scrolls or resizes, the pointer moves, the cue moves, the theme changes, or the page shifts under it", async () => {
    const { canvas, frames, asked, until } = clock();
    const wakes: [string, () => void][] = [
      ["scroll", () => window.dispatchEvent(new Event("scroll"))],
      ["resize", () => window.dispatchEvent(new Event("resize"))],
      ["pointer", () => window.dispatchEvent(Object.assign(new Event("pointermove"), { pointerType: "mouse", clientX: 10, clientY: 10 }))],
      ["cue", () => document.querySelector("[data-slot=long-page]")!.dispatchEvent(new Event(CUE_MOVED, { bubbles: true }))],
      ["transition", () => document.querySelector("[data-reveal]")!.dispatchEvent(new Event("transitionstart", { bubbles: true }))],
      ["theme", () => document.documentElement.setAttribute("data-mode", document.documentElement.dataset.mode === "dark" ? "light" : "dark")],
    ];
    const standing = [4000, 6000, 10000, 16500, 18000, 19500];
    for (const [i, [what, wake]] of wakes.entries()) {
      until(standing[i]!);
      expect(canvas.dataset.owl, `standing before the ${what}`).toBe("perched");
      frames.mockClear();
      wake();
      await act(() => Promise.resolve());
      expect(asked(), `the ${what} wakes it`).toBe(1);
    }
    document.documentElement.removeAttribute("data-mode");
  }, 30_000);

  it("a touch does not wake it, as it has no pointer to follow", () => {
    const { canvas, frames, asked, until } = clock();
    until(4000);
    expect(canvas.dataset.owl).toBe("perched");
    frames.mockClear();
    window.dispatchEvent(Object.assign(new Event("pointermove"), { pointerType: "touch", clientX: 10, clientY: 10 }));
    expect(asked()).toBe(0);
  });

  it("bobs on the air in CSS as it flies and stands, by its own size, and only with motion allowed and not while peeking", () => {
    const { canvas, until } = clock();
    until(100);
    expect(canvas.style.getPropertyValue("--owl-bob")).toBe(`${(owlSize(window.innerWidth) * 0.035).toFixed(2)}px`);
    const css = readFileSync(join(__dirname, "scroll.module.css"), "utf8");
    const at = css.indexOf("@media (prefers-reduced-motion: no-preference)");
    expect(css.indexOf("owl-bob 3.27s"), "the bob runs with motion allowed").toBeGreaterThan(at);
    expect(css.slice(0, at)).not.toContain("animation: owl-bob");
    expect(css).toMatch(/\.owl:is\(\[data-owl="flying"\], \[data-owl="perched"\]\) \{\s*animation: owl-bob/);
    expect(css).toContain("translate: 0 calc(-1 * var(--owl-bob))");
  });
});

describe("its perches", () => {
  it("stands on an element's top edge, the given way across, with its feet on the line", () => {
    const el = document.createElement("div");
    el.dataset.perchAt = "0.25";
    el.dataset.perchYaw = "0.5";
    box(el, { left: 100, top: 400, width: 800, height: 300, bottom: 700 });
    expect(perchOn(el, 100)).toEqual({ x: 300, y: 400 - 50, yaw: 0.5, coat: "ink" });
    expect(perchOn(el, 100, { left: 50, top: 100 })).toMatchObject({ x: 250, y: 300 - 50 });
    el.dataset.perchCoat = "pale";
    expect(perchOn(el, 100).coat).toBe("pale");
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

  it("wears its pale coat exactly where it stands on tide or the opening's pixel sea, so its ink never sinks into the teal", () => {
    const { container } = render(<LongPage />);
    const overSea = (el: HTMLElement) => {
      for (let a = el.parentElement; a; a = a.parentElement) if ([...a.children].some((c) => c.matches("[data-slot=pixel-sea]"))) return true;
      return false;
    };
    for (const p of container.querySelectorAll<HTMLElement>("[data-perch]")) {
      const onTide = p.closest(".bg-tide") !== null || overSea(p);
      expect(p.dataset.perchCoat, p.outerHTML.slice(0, 80)).toBe(onTide ? "pale" : "ink");
    }
  });
});
