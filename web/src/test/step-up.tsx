import { act, fireEvent, screen, within } from "@testing-library/react";
import { expect, vi } from "vitest";
import { PASSKEY_ANSWER_MS, type Passkey } from "@/components/stop/step-up-dialog";
import { useRuntime } from "@/lib/mock-runtime";
import { RECORD_AFTER_MS } from "./harness";

/** Shared by the Stop sheet and the record screens: both open G3 and report commands the same way. */

export const failingPasskey: Passkey = (_action, answer) => {
  const id = window.setTimeout(() => answer("failed"), PASSKEY_ANSWER_MS);
  return () => window.clearTimeout(id);
};

/** Answers "verified" even after being abandoned, as a browser's assertion may resolve late. */
export const latePasskey: Passkey = (_action, answer) => {
  window.setTimeout(() => answer("verified"), PASSKEY_ANSWER_MS);
  return () => {};
};

export function stepUpDialog() {
  return screen.queryByRole("dialog", { name: "Confirm it is you" });
}

export function press(name: "Use passkey" | "Cancel" | "Close") {
  fireEvent.click(within(stepUpDialog()!).getByRole("button", { name }));
}

export function log(root: HTMLElement) {
  return within(root).getByRole("status");
}

export function entries(root: HTMLElement) {
  return log(root).querySelectorAll("[data-phase]");
}

/** The modes shown for every agent in view: they change only once the runtime records a command. */
export function modes(root: HTMLElement) {
  return Array.from(root.querySelectorAll("[data-slot=mode-badge]"), (b) => b.getAttribute("data-mode"));
}

export function literal(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Nothing reached the runtime: no entry now or later, and every agent keeps its mode. */
export function expectNothingSent(root: HTMLElement, before: Array<string | null>) {
  expect(entries(root)).toHaveLength(0);
  act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));
  expect(entries(root)).toHaveLength(0);
  expect(modes(root)).toEqual(before);
}

/** Rule 5: "sent" first, the recorded state only once the runtime records it, and exactly one command. */
export function expectSentThenRecorded(root: HTMLElement, before: Array<string | null>, recorded: string) {
  expect(entries(root)).toHaveLength(1);
  expect(entries(root)[0]).toHaveAttribute("data-phase", "sent");
  expect(log(root)).toHaveTextContent("Sent; waiting for the runtime to record it.");
  expect(log(root)).not.toHaveTextContent("Recorded");
  expect(log(root)).not.toHaveTextContent(recorded);
  expect(modes(root)).toEqual(before);

  act(() => vi.advanceTimersByTime(RECORD_AFTER_MS - 1));
  expect(entries(root)[0]).toHaveAttribute("data-phase", "sent");
  expect(modes(root)).toEqual(before);

  act(() => vi.advanceTimersByTime(1));
  expect(entries(root)).toHaveLength(1);
  expect(entries(root)[0]).toHaveAttribute("data-phase", "recorded");
  expect(log(root)).toHaveTextContent(new RegExp(`Recorded at \\d{2}:\\d{2}:\\d{2}\\. ${literal(recorded)}`));
  expect(log(root)).not.toHaveTextContent("Sent; waiting");
  expect(modes(root)).not.toEqual(before);
}

/**
 * A press beside the passkey dialog. Nested in the Stop sheet it lands on Base UI's own backdrop;
 * opened from a page it lands on the dialog's styled backdrop.
 */
export function clickOutside() {
  act(() => vi.advanceTimersByTime(0));
  const portal = stepUpDialog()!.parentElement!;
  const overlay = portal.querySelector(":scope > [role=presentation]") ?? portal.querySelector(":scope > [data-slot=sheet-backdrop]")!;
  fireEvent.pointerDown(overlay);
  fireEvent.mouseDown(overlay);
  fireEvent.click(overlay);
}

export interface Ending {
  ending: string;
  notice: string;
  passkey?: Passkey;
  run: () => void;
}

/** Every way a passkey check can end without a verified answer to the request still in flight. */
export const ENDINGS: Ending[] = [
  { ending: "Cancel", notice: "Passkey check canceled. Nothing was sent.", run: () => press("Cancel") },
  { ending: "Escape", notice: "Passkey check canceled. Nothing was sent.", run: () => fireEvent.keyDown(stepUpDialog()!, { key: "Escape" }) },
  {
    ending: "Cancel while the passkey is being asked",
    notice: "Passkey check canceled. Nothing was sent.",
    run: () => {
      press("Use passkey");
      act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS - 1));
      press("Cancel");
    },
  },
  {
    ending: "Escape while the passkey is being asked",
    notice: "Passkey check canceled. Nothing was sent.",
    run: () => {
      press("Use passkey");
      act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS - 1));
      fireEvent.keyDown(stepUpDialog()!, { key: "Escape" });
    },
  },
  {
    ending: "Cancel after pressing Use passkey twice",
    notice: "Passkey check canceled. Nothing was sent.",
    run: () => {
      press("Use passkey");
      press("Use passkey");
      press("Cancel");
    },
  },
  {
    ending: "Cancel before a late verified answer",
    notice: "Passkey check canceled. Nothing was sent.",
    passkey: latePasskey,
    run: () => {
      press("Use passkey");
      press("Cancel");
    },
  },
  { ending: "the dialog's Close button", notice: "Passkey check canceled. Nothing was sent.", run: () => press("Close") },
  {
    ending: "the Close button before a late verified answer",
    notice: "Passkey check canceled. Nothing was sent.",
    passkey: latePasskey,
    run: () => {
      press("Use passkey");
      press("Close");
    },
  },
  { ending: "a click outside the dialog", notice: "Passkey check canceled. Nothing was sent.", run: () => clickOutside() },
  {
    ending: "a click outside before a late verified answer",
    notice: "Passkey check canceled. Nothing was sent.",
    passkey: latePasskey,
    run: () => {
      press("Use passkey");
      clickOutside();
    },
  },
  {
    ending: "a failed passkey check",
    notice: "Passkey check failed. Nothing was sent.",
    passkey: failingPasskey,
    run: () => {
      press("Use passkey");
      act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    },
  },
];

/** What the runtime received, rendered beside the screen under test so it outlives it. */
export function RuntimeCommands() {
  const { commands } = useRuntime();
  return (
    <output data-slot="runtime-commands" data-commands={JSON.stringify(commands)}>
      {commands.length}
    </output>
  );
}

export function recordedCount() {
  return Number(document.querySelector("[data-slot=runtime-commands]")?.textContent);
}

export function runtimeCommands(): Array<Record<string, unknown>> {
  return JSON.parse(document.querySelector("[data-slot=runtime-commands]")?.getAttribute("data-commands") ?? "[]");
}
