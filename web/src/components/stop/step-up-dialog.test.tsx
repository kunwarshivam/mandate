import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { heldPasskey, watchConsole } from "@/test/passkey";
import { PASSKEY_ANSWER_MS, type Passkey, type PasskeyResult, PasskeyContext, StepUpDialog } from "./step-up-dialog";

const ACTION = "Kill switch for Agent 1: cancel its orders, sell its positions, and end it.";

function renderDialog(passkey: Passkey, open = true) {
  const handlers = { onVerified: vi.fn(), onCancel: vi.fn(), onFailed: vi.fn() };
  const ui = (isOpen: boolean) => (
    <PasskeyContext.Provider value={passkey}>
      <StepUpDialog action={ACTION} open={isOpen} {...handlers} />
    </PasskeyContext.Provider>
  );
  const view = render(ui(open));
  return { ...view, handlers, reopen: (isOpen: boolean) => view.rerender(ui(isOpen)) };
}

function dialog() {
  return screen.getByRole("dialog", { name: "Confirm it is you" });
}

let console_: ReturnType<typeof watchConsole>;

beforeEach(() => {
  vi.useFakeTimers();
  console_ = watchConsole();
});

afterEach(() => {
  console_.restore();
  vi.useRealTimers();
});

describe.each<PasskeyResult>(["verified", "failed"])("StepUpDialog, with a %s answer still to come", (result) => {
  it("abandons the request on unmount and never answers after it", () => {
    const { passkey, release } = heldPasskey(result);
    const { handlers, unmount } = renderDialog(passkey);
    fireEvent.click(within(dialog()).getByRole("button", { name: "Use passkey" }));
    expect(passkey).toHaveBeenCalledTimes(1);

    unmount();
    expect(release).toHaveBeenCalledTimes(1);
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS * 3));

    expect(handlers.onVerified).not.toHaveBeenCalled();
    expect(handlers.onFailed).not.toHaveBeenCalled();
    expect(handlers.onCancel).not.toHaveBeenCalled();
    expect(console_.calls()).toEqual([]);
  });

  it("abandons the request when the parent closes the dialog, and never answers after it", () => {
    const { passkey, release } = heldPasskey(result);
    const { handlers, reopen } = renderDialog(passkey);
    fireEvent.click(within(dialog()).getByRole("button", { name: "Use passkey" }));

    reopen(false);
    expect(release).toHaveBeenCalledTimes(1);
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS * 3));

    expect(handlers.onVerified).not.toHaveBeenCalled();
    expect(handlers.onFailed).not.toHaveBeenCalled();
    expect(console_.calls()).toEqual([]);
  });

  it("opens again idle after the parent closed it mid-request", () => {
    const { passkey } = heldPasskey(result);
    const { handlers, reopen } = renderDialog(passkey);
    fireEvent.click(within(dialog()).getByRole("button", { name: "Use passkey" }));
    reopen(false);
    reopen(true);
    expect(dialog()).not.toHaveTextContent("Waiting for your passkey…");

    fireEvent.click(within(dialog()).getByRole("button", { name: "Use passkey" }));
    expect(passkey).toHaveBeenCalledTimes(2);
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    expect(result === "verified" ? handlers.onVerified : handlers.onFailed).toHaveBeenCalledTimes(1);
    expect(console_.calls()).toEqual([]);
  });
});

describe("StepUpDialog before any request", () => {
  it("unmounts without asking the passkey or answering", () => {
    const { passkey, release } = heldPasskey("verified");
    const { handlers, unmount } = renderDialog(passkey);
    unmount();
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS * 3));
    expect(passkey).not.toHaveBeenCalled();
    expect(release).not.toHaveBeenCalled();
    expect(handlers.onVerified).not.toHaveBeenCalled();
    expect(console_.calls()).toEqual([]);
  });
});
