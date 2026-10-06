import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import { AGENT_IDS } from "@/fixtures/workspace";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { InlineDisclosures, Placeholder } from "./placeholders";

/**
 * DEC-210: the performance disclosure is an info symbol beside each P&L that opens the disclosure
 * text on hover, click, tap, or focus and Enter or Space; Escape closes it and gives focus back. The
 * text is the symbol's description, so a screen reader hears it without opening. Where nothing can be
 * opened, the text shows in full, as the other placeholders always do.
 */

const TEXT = "[[DISCLOSURE-PERFORMANCE]]";
const trigger = () => screen.getByRole("button", { name: "Performance disclosure" });
const popover = () => screen.queryByRole("dialog", { name: "Performance disclosure" });

afterEach(() => vi.useRealTimers());

function Row() {
  return (
    <p>
      Paper P&amp;L, simulated <span data-direction="gain">+$12.34</span> since deployed <Placeholder name="performance" />
    </p>
  );
}

describe("the performance disclosure symbol", () => {
  it("is a button named Performance disclosure, described by the disclosure text beside it", () => {
    const { container } = render(<Row />);
    const button = trigger();
    expect(button.tagName).toBe("BUTTON");
    expect(button).toHaveAttribute("type", "button");
    expect(button).toHaveAccessibleDescription(TEXT);
    const text = document.getElementById(button.getAttribute("aria-describedby")!)!;
    expect(text).toHaveAttribute("data-placeholder", "performance");
    expect(text).toHaveTextContent(TEXT);
    expect(container.querySelector("p")!.contains(text)).toBe(true);
    expect(container.querySelectorAll("[data-placeholder=performance]")).toHaveLength(1);
    expect(popover()).toBeNull();
  });

  it("uses the icon set's info glyph, muted, on its 24px grid, as its own 24px target that grows to 44px on touch", () => {
    render(<Row />);
    const button = trigger();
    const glyph = button.querySelector("svg")!;
    expect(glyph).toHaveAttribute("aria-hidden", "true");
    expect(glyph.getAttribute("class")).toMatch(/\bsize-6\b/);
    expect(button).toHaveClass("text-muted-foreground", "w-6", "before:size-6", "pointer-coarse:before:size-11");
  });

  it("opens on click and closes on a second click", async () => {
    render(<Row />);
    fireEvent.click(trigger());
    const dialog = await screen.findByRole("dialog", { name: "Performance disclosure" });
    const text = within(dialog).getByText(TEXT);
    expect(text).toHaveAttribute("data-placeholder", "performance");
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(trigger());
    await waitFor(() => expect(popover()).toBeNull());
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
  });

  it("closes on a press outside it, as a tap outside does on a phone", async () => {
    render(
      <>
        <Row />
        <p>Elsewhere</p>
      </>,
    );
    fireEvent.click(trigger());
    await screen.findByRole("dialog", { name: "Performance disclosure" });
    const outside = screen.getByText("Elsewhere");
    fireEvent.pointerDown(outside, { pointerType: "touch" });
    fireEvent.mouseDown(outside);
    fireEvent.pointerUp(outside, { pointerType: "touch" });
    fireEvent.click(outside);
    await waitFor(() => expect(popover()).toBeNull());
  });

  it("closes on Escape and gives focus back to the symbol (Enter and Space are a native button's; e2e presses them)", async () => {
    render(<Row />);
    const button = trigger();
    act(() => button.focus());
    fireEvent.click(button);
    const dialog = await screen.findByRole("dialog", { name: "Performance disclosure" });
    fireEvent.keyDown(document.activeElement ?? dialog, { key: "Escape" });
    await waitFor(() => expect(popover()).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(trigger()));
  });

  it("does not open on focus alone", async () => {
    render(<Row />);
    act(() => trigger().focus());
    await act(() => new Promise((r) => setTimeout(r, 300)));
    expect(popover()).toBeNull();
  });

  it("opens on hover after a short delay and closes when the pointer leaves", async () => {
    vi.useFakeTimers();
    render(<Row />);
    const button = trigger();
    fireEvent.pointerEnter(button, { pointerType: "mouse" });
    fireEvent.mouseEnter(button);
    fireEvent.mouseMove(button);
    expect(popover()).toBeNull();
    await act(async () => vi.advanceTimersByTime(200));
    expect(popover()).not.toBeNull();
    fireEvent.pointerLeave(button, { pointerType: "mouse" });
    fireEvent.mouseLeave(button);
    await act(async () => vi.advanceTimersByTime(1000));
    expect(popover()).toBeNull();
  });

  it("hides the symbol in print and prints the full text in its place", () => {
    render(<Row />);
    expect(trigger()).toHaveClass("print:hidden");
    const text = document.getElementById(trigger().getAttribute("aria-describedby")!)!;
    expect(text).toHaveClass("not-print:sr-only", "border-dashed");
    expect(text.className).not.toMatch(/(^|\s)sr-only(\s|$)/);
  });
});

describe("where the disclosure shows in full", () => {
  it("shows the full text inline, and no symbol, inside InlineDisclosures", () => {
    const { container } = render(
      <InlineDisclosures>
        <Row />
      </InlineDisclosures>,
    );
    expect(screen.queryByRole("button")).toBeNull();
    const tag = container.querySelector("[data-placeholder=performance]")!;
    expect(tag).toHaveTextContent(TEXT);
    expect(tag).toHaveClass("border-dashed");
    expect(tag.className).not.toMatch(/sr-only/);
  });

  it("shows it inline on a frozen record screen, where collapsed content counts as not shown, and as a symbol elsewhere", () => {
    setPathname(`/agents/${AGENT_IDS.swing}/kill-switch`);
    const { unmount } = renderWithRuntime(
      <AppShell>
        <Row />
      </AppShell>,
    );
    expect(within(screen.getByRole("main")).queryByRole("button", { name: "Performance disclosure" })).toBeNull();
    expect(screen.getByRole("main").querySelector("[data-placeholder=performance]")).toHaveTextContent(TEXT);
    unmount();

    setPathname(`/agents/${AGENT_IDS.swing}`);
    renderWithRuntime(
      <AppShell>
        <Row />
      </AppShell>,
    );
    expect(within(screen.getByRole("main")).getByRole("button", { name: "Performance disclosure" })).toHaveAccessibleDescription(TEXT);
  });

  it.each([
    ["hypothetical", "[[LEGEND-HYPOTHETICAL]]"],
    ["retailAutoLive", "[[RETAIL-AUTO-LIVE]]"],
  ] as const)("leaves the %s placeholder as a visible tag", (name, text) => {
    const { container } = render(<Placeholder name={name} />);
    expect(screen.queryByRole("button")).toBeNull();
    const tag = container.querySelector(`[data-placeholder=${name}]`);
    expect(tag).toHaveTextContent(text);
    expect(tag).toHaveClass("border-dashed");
  });

  it("goes back to the inline tag everywhere with one line: a single constant in placeholders.tsx", () => {
    const source = readFileSync(join(__dirname, "placeholders.tsx"), "utf8");
    expect(source.match(/^const PERFORMANCE_INLINE = (true|false);$/gm)).toEqual(["const PERFORMANCE_INLINE = false;"]);
    expect(source.match(/PERFORMANCE_INLINE/g)).toHaveLength(2);
  });
});
