import type { ReactElement } from "react";
import { render } from "@testing-library/react";
import { Providers } from "@/components/providers";
import { type Passkey, PasskeyContext, mockPasskey } from "@/components/stop/step-up-dialog";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { type Role, RoleProvider } from "@/lib/roles";

export const RECORD_AFTER_MS = 1600;

/**
 * Renders with the same providers as the app (links, toasts, runtime), with the clock held still.
 * `rerender` swaps the UI inside the same runtime, so the runtime outlives what it replaces.
 */
export function renderWithRuntime(
  ui: ReactElement,
  scenario: Scenario = "normal",
  { role = "owner", passkey = mockPasskey }: { role?: Role; passkey?: Passkey } = {},
) {
  const initial = buildWorkspace(scenario);
  const wrap = (inner: ReactElement) => (
    <PasskeyContext.Provider value={passkey}>
      <RoleProvider initial={role}>
        <Providers workspace={initial} tick={false} recordAfterMs={RECORD_AFTER_MS}>
          {inner}
        </Providers>
      </RoleProvider>
    </PasskeyContext.Provider>
  );
  const view = render(wrap(ui));
  return { ...view, rerender: (next: ReactElement) => view.rerender(wrap(next)) };
}

/** A control counts as disabled if the browser or assistive technology would treat it so. */
export function isDisabled(el: Element): boolean {
  return el.hasAttribute("disabled") || el.getAttribute("aria-disabled") === "true" || el.closest("fieldset[disabled]") !== null;
}
