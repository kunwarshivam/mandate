import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { JumpToLatest, useFollow } from "./follow";

const VIEW = 400;
const CONTENT = 2000;

function Log() {
  const { scroller, content, away, jump } = useFollow("thread");
  return (
    <>
      <div ref={scroller} data-testid="log">
        <div ref={content} />
      </div>
      <JumpToLatest away={away} onJump={jump} />
    </>
  );
}

/** jsdom lays nothing out, so the log is given a fixed frame and content height, and its scroll position is set by hand. */
function log() {
  render(<Log />);
  const el = screen.getByTestId("log");
  Object.defineProperty(el, "clientHeight", { configurable: true, value: VIEW });
  Object.defineProperty(el, "scrollHeight", { configurable: true, value: CONTENT });
  const moveTo = (top: number) => {
    el.scrollTop = top;
    fireEvent.scroll(el);
  };
  moveTo(CONTENT - VIEW);
  return { el, moveTo };
}

const jump = () => screen.queryByRole("button", { name: "Jump to latest" });

describe("a log that follows the latest entry", () => {
  it("keeps following when its scroll moves up with no reader behind it, as a frame resize or shrinking content clamps it", () => {
    const { moveTo } = log();
    moveTo(CONTENT - VIEW - 600);
    expect(jump()).toBeNull();
  });

  it("stops following when the reader wheels up, and offers the way back", () => {
    const { el, moveTo } = log();
    fireEvent.wheel(el, { deltaY: -300 });
    moveTo(CONTENT - VIEW - 600);
    expect(jump()).toBeInTheDocument();
  });

  it("ignores a wheel down, which never leaves the latest entry", () => {
    const { el, moveTo } = log();
    fireEvent.wheel(el, { deltaY: 300 });
    moveTo(CONTENT - VIEW - 600);
    expect(jump()).toBeNull();
  });

  it("stops following when a finger drags it up, and through the momentum after the finger lifts", () => {
    const { el, moveTo } = log();
    fireEvent.touchStart(el);
    fireEvent.touchEnd(el);
    moveTo(CONTENT - VIEW - 100);
    expect(jump()).toBeInTheDocument();
  });

  it("stops following on Page Up", () => {
    const { moveTo } = log();
    fireEvent.keyDown(window, { key: "PageUp" });
    moveTo(CONTENT - VIEW - 400);
    expect(jump()).toBeInTheDocument();
  });

  it("follows again once the reader is back at the end", () => {
    const { el, moveTo } = log();
    fireEvent.wheel(el, { deltaY: -300 });
    moveTo(0);
    expect(jump()).toBeInTheDocument();
    act(() => moveTo(CONTENT - VIEW - 10));
    expect(jump()).toBeNull();
  });
});
