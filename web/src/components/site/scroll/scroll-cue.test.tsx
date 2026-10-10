import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { INTRO_ID } from "./parts";
import { CUE_MOVED, ScrollCue } from "./scroll-cue";

/**
 * The cue on the desktop's first screen that there is more below (DEC-907): a link to the long page,
 * in the middle of the screen, on the row of the browser window's status bar when that bar runs under
 * the middle, or above the bottom edge when it does not.
 */

const rect = (r: Partial<DOMRect>) => ({ left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}), ...r }) as DOMRect;

/** A 1440 by 900 desktop whose window's status bar runs from `from` to `to` across, at 840 down. */
function layout(from = 200, to = 1240) {
  vi.spyOn(window, "requestAnimationFrame").mockReturnValue(0);
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    switch (this.getAttribute("data-slot")) {
      case "desktop-stage":
        return rect({ width: 1440, height: 900, right: 1440, bottom: 900 });
      case "status-text":
        return rect({ left: from, right: to, top: 840, width: to - from, height: 24 });
      default:
        return rect({});
    }
  });
}

function stage(withStatus: boolean) {
  return render(
    <div data-slot="desktop-stage">
      {withStatus && <span data-slot="status-text">Document: Done</span>}
      <ScrollCue />
      <section id={INTRO_ID} />
    </div>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("the scroll cue", () => {
  it("is a link named Scroll to the start of the long page", () => {
    layout();
    stage(true);
    expect(screen.getByRole("link", { name: "Scroll" })).toHaveAttribute("href", `#${INTRO_ID}`);
  });

  it("sits in the middle of the screen, on the row of a status bar that runs under the middle", () => {
    layout(200, 1240);
    stage(true);
    const cue = screen.getByRole("link", { name: "Scroll" });
    expect(cue.dataset.at).toBe("status");
    expect([cue.style.left, cue.style.top]).toEqual(["720px", "852px"]);
  });

  it("stays in the middle of the screen when the window sits off to one side, above the bottom edge", () => {
    layout(200, 600);
    stage(true);
    const cue = screen.getByRole("link", { name: "Scroll" });
    expect(cue.dataset.at).toBe("edge");
    expect([cue.style.left, cue.style.top]).toEqual(["720px", "836px"]);
  });

  it("sits above the bottom edge, in the middle, when no window's status bar shows", () => {
    layout();
    stage(false);
    const cue = screen.getByRole("link", { name: "Scroll" });
    expect(cue.dataset.at).toBe("edge");
    expect([cue.style.left, cue.style.top]).toEqual(["720px", "836px"]);
  });

  it("stops measuring once it has stayed put for 30 frames, and measures again only when the window holding the status bar changes or animates", async () => {
    layout(200, 1240);
    const queue: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => queue.push(cb));
    const run = () => {
      let n = 0;
      while (queue.length > 0 && n < 1000) {
        queue.shift()!(n * 16);
        n++;
      }
      return n;
    };
    render(
      <div data-slot="desktop-stage">
        <section data-slot="os-window">
          <span data-slot="status-text">Document: Done</span>
        </section>
        <ul data-slot="desktop-icons" />
        <ScrollCue />
        <section id={INTRO_ID} />
      </div>,
    );
    const cue = screen.getByRole("link", { name: "Scroll" });
    expect(run(), "frames until it has stayed put for 30").toBe(30);
    const settle = () => act(() => Promise.resolve());

    document.querySelector<HTMLElement>("[data-slot=desktop-icons]")!.style.translate = "4px 0";
    cue.style.outline = "1px solid";
    await settle();
    expect(queue, "neither an icon nor the cue itself moves it").toHaveLength(0);

    const moved = vi.fn();
    document.addEventListener(CUE_MOVED, moved);
    vi.mocked(Element.prototype.getBoundingClientRect).mockImplementation(function (this: Element) {
      if (this.getAttribute("data-slot") === "desktop-stage") return rect({ width: 1440, height: 900, right: 1440, bottom: 900 });
      if (this.getAttribute("data-slot") === "status-text") return rect({ left: 200, right: 1240, top: 600, width: 1040, height: 24 });
      return rect({});
    });
    document.querySelector<HTMLElement>("[data-slot=os-window]")!.style.translate = "0 -240px";
    await settle();
    expect(queue, "the window holding the status bar moved").toHaveLength(1);
    expect(run()).toBe(31);
    expect(cue.style.top).toBe("612px");
    expect(moved, "it tells the owl it moved").toHaveBeenCalledTimes(1);

    document.querySelector("[data-slot=os-window]")!.dispatchEvent(new Event("animationstart", { bubbles: true }));
    expect(queue, "the window began to animate").toHaveLength(1);
    expect(run()).toBe(30);
    expect(moved, "it stayed put, so it says nothing").toHaveBeenCalledTimes(1);
    document.removeEventListener(CUE_MOVED, moved);
  });

  it("holds its arrow still once the page has covered it, and lets it go when the first screen comes back", () => {
    layout();
    stage(true);
    const cue = screen.getByRole("link", { name: "Scroll" });
    expect(cue.dataset.ambient).toBe("playing");
    vi.spyOn(window, "scrollY", "get").mockReturnValue(window.innerHeight);
    window.dispatchEvent(new Event("scroll"));
    expect(cue.dataset.ambient).toBe("paused");
    vi.spyOn(window, "scrollY", "get").mockReturnValue(0);
    window.dispatchEvent(new Event("scroll"));
    expect(cue.dataset.ambient).toBe("playing");
  });

  it("brings the long page into view when pressed, without changing the address", () => {
    layout();
    const into = vi.spyOn(Element.prototype, "scrollIntoView");
    stage(true);
    expect(fireEvent.click(screen.getByRole("link", { name: "Scroll" }))).toBe(false);
    expect(into.mock.contexts[0]).toBe(document.getElementById(INTRO_ID));
  });
});
