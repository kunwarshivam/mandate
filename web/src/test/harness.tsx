import type { ReactElement } from "react";
import { render } from "@testing-library/react";
import { type Passkey, PasskeyContext, mockPasskey } from "@/components/stop/step-up-dialog";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { RuntimeProvider } from "@/lib/mock-runtime";

export const RECORD_AFTER_MS = 1600;

/** `rerender` swaps the UI inside the same runtime, so the runtime outlives what it replaces. */
export function renderWithRuntime(ui: ReactElement, scenario: Scenario = "normal", { passkey = mockPasskey }: { passkey?: Passkey } = {}) {
  const initial = buildWorkspace(scenario);
  const wrap = (inner: ReactElement) => (
    <PasskeyContext.Provider value={passkey}>
      <RuntimeProvider initial={initial} tick={false} recordAfterMs={RECORD_AFTER_MS}>
        {inner}
      </RuntimeProvider>
    </PasskeyContext.Provider>
  );
  const view = render(wrap(ui));
  return { ...view, rerender: (next: ReactElement) => view.rerender(wrap(next)) };
}

/** A control counts as disabled if the browser or assistive technology would treat it so. */
export function isDisabled(el: Element): boolean {
  return el.hasAttribute("disabled") || el.getAttribute("aria-disabled") === "true" || el.closest("fieldset[disabled]") !== null;
}
