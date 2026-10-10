import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { INTRO_ID } from "./parts";
import { ScrollCue } from "./scroll-cue";

/**
 * The cue on the desktop's first screen that there is more below (DEC-907): a link to the long page,
 * on the middle of the browser window's status bar, or above the bottom edge when no window shows.
 */

const rect = (r: Partial<DOMRect>) => ({ left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}), ...r }) as DOMRect;

/** A 1440 by 900 desktop whose window's status bar runs from 200 to 800 across, at 840 down. */
function layout() {
  vi.spyOn(window, "requestAnimationFrame").mockReturnValue(0);
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    switch (this.getAttribute("data-slot")) {
      case "desktop-stage":
        return rect({ width: 1440, height: 900 });
      case "status-text":
        return rect({ left: 200, top: 840, width: 600, height: 24 });
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

  it("sits on the middle of the window's status bar", () => {
    layout();
    stage(true);
    const cue = screen.getByRole("link", { name: "Scroll" });
    expect(cue.dataset.at).toBe("status");
    expect([cue.style.left, cue.style.top]).toEqual(["500px", "852px"]);
  });

  it("sits above the bottom edge, in the middle, when no window's status bar shows", () => {
    layout();
    stage(false);
    const cue = screen.getByRole("link", { name: "Scroll" });
    expect(cue.dataset.at).toBe("edge");
    expect([cue.style.left, cue.style.top]).toEqual(["720px", "836px"]);
  });

  it("brings the long page into view when pressed, without changing the address", () => {
    layout();
    const into = vi.spyOn(Element.prototype, "scrollIntoView");
    stage(true);
    expect(fireEvent.click(screen.getByRole("link", { name: "Scroll" }))).toBe(false);
    expect(into.mock.contexts[0]).toBe(document.getElementById(INTRO_ID));
  });
});
