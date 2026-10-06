import type { ReactElement } from "react";
import { render, screen, within } from "@testing-library/react";
import { Providers } from "@/components/providers";
import { type Passkey, PasskeyContext, mockPasskey } from "@/components/stop/step-up-dialog";
import type { Scenario, Workspace } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { type Role, RoleProvider } from "@/lib/roles";
import { type Session, SessionProvider } from "@/lib/session";

export const RECORD_AFTER_MS = 1600;

/**
 * Renders with the same providers as the app (links, toasts, runtime), with the clock held still.
 * `rerender` swaps the UI inside the same runtime, so the runtime outlives what it replaces.
 */
export function renderWithRuntime(
  ui: ReactElement,
  scenario: Scenario = "normal",
  {
    role = "owner",
    passkey = mockPasskey,
    session = null,
    workspace,
  }: { role?: Role; passkey?: Passkey; session?: Session | null; workspace?: (ws: Workspace) => Workspace } = {},
) {
  const built = buildWorkspace(scenario);
  const initial = workspace ? workspace(built) : built;
  const wrap = (inner: ReactElement) => (
    <SessionProvider session={session}>
      <PasskeyContext.Provider value={passkey}>
        <RoleProvider initial={role}>
          <Providers workspace={initial} tick={false} recordAfterMs={RECORD_AFTER_MS}>
            {inner}
          </Providers>
        </RoleProvider>
      </PasskeyContext.Provider>
    </SessionProvider>
  );
  const view = render(wrap(ui));
  return { ...view, rerender: (next: ReactElement) => view.rerender(wrap(next)) };
}

/**
 * The desktop dock's Stop. The shell renders it twice, at the end of the dock and of the phone tab
 * bar, and jsdom applies no breakpoints, so a test picks one by its navigation (DEC-452).
 */
export function dockStop(): HTMLElement {
  return within(screen.getByRole("navigation", { name: "Primary" })).getByRole("button", { name: "Stop" });
}

/** The phone tab bar's Stop. */
export function tabStop(): HTMLElement {
  return within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name: "Stop" });
}

/** A control counts as disabled if the browser or assistive technology would treat it so. */
export function isDisabled(el: Element): boolean {
  return el.hasAttribute("disabled") || el.getAttribute("aria-disabled") === "true" || el.closest("fieldset[disabled]") !== null;
}
