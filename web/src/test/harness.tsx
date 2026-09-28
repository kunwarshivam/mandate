import type { ReactElement } from "react";
import { render } from "@testing-library/react";
import { Providers } from "@/components/providers";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { type Role, RoleProvider } from "@/lib/roles";

export const RECORD_AFTER_MS = 1600;

/** Renders with the same providers as the app (links, toasts, runtime), with the clock held still. */
export function renderWithRuntime(ui: ReactElement, scenario: Scenario = "normal", role: Role = "owner") {
  return render(
    <RoleProvider initial={role}>
      <Providers workspace={buildWorkspace(scenario)} tick={false} recordAfterMs={RECORD_AFTER_MS}>
        {ui}
      </Providers>
    </RoleProvider>,
  );
}

/** A control counts as disabled if the browser or assistive technology would treat it so. */
export function isDisabled(el: Element): boolean {
  return el.hasAttribute("disabled") || el.getAttribute("aria-disabled") === "true" || el.closest("fieldset[disabled]") !== null;
}
