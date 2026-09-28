import { vi } from "vitest";
import { PASSKEY_ANSWER_MS, type Passkey, type PasskeyResult } from "@/components/stop/step-up-dialog";

/**
 * A passkey that keeps its answer callback and answers after PASSKEY_ANSWER_MS whether or not it
 * was abandoned, the way a browser's assertion can resolve after the page stopped waiting for it.
 */
export function heldPasskey(result: PasskeyResult) {
  const release = vi.fn();
  const passkey = vi.fn<Passkey>((_action, answer) => {
    window.setTimeout(() => answer(result), PASSKEY_ANSWER_MS);
    return release;
  });
  return { passkey, release };
}

/** React reports state updates it cannot apply, and updates outside act, through console.error. */
export function watchConsole() {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  return {
    calls: () => [...error.mock.calls, ...warn.mock.calls],
    restore: () => {
      error.mockRestore();
      warn.mockRestore();
    },
  };
}
