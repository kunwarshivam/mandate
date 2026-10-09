import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { StopSheetHost } from "@/components/shell/stop-control";
import type { RestrictionCode } from "@/fixtures/types";
import { AGENT_IDS } from "@/fixtures/workspace";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { stepUpDialog } from "@/test/step-up";
import { ModeBanner } from "./mode";

const SINCE = "2026-09-28T14:01:12-04:00";

/** The "Who acts" line of the one restriction in the banner. */
function whoActs(): HTMLElement {
  const banner = screen.getByRole("region", { name: "Restrictions" });
  const term = within(banner)
    .getAllByRole("term")
    .find((t) => t.textContent === "Who acts");
  expect(term, "the banner has a Who acts line").toBeDefined();
  const definition = term!.nextElementSibling as HTMLElement;
  expect(definition).toHaveRole("definition");
  return definition;
}

function banner(code: RestrictionCode, role?: "owner" | "operator" | "approver" | "viewer") {
  return renderWithRuntime(<ModeBanner mode="paused" restrictions={[{ code, since: SINCE }]} />, "normal", role ? { role } : undefined);
}

/**
 * Where the owner ends each restriction only they can end (critique C-24), worked out from the
 * built app rather than from the table under test: Resume and release are in the Stop sheet, each
 * behind a passkey; acknowledgments (D7), the connection page (O4) and changing instruments are not
 * built yet, so they are named without a door.
 */
const IN_STOP: readonly RestrictionCode[] = ["owner_pause", "goal_complete"];
const COMING: ReadonlyArray<[RestrictionCode, RegExp]> = [
  ["drawdown_exits_only", /Overview/],
  ["drawdown_flatten", /Overview/],
  ["daily_loss", /Overview/],
  ["reconciliation_mismatch", /Connections/],
  ["external_activity", /Connections/],
  ["account_closing_only", /Connections/],
  ["account_blocked", /Connections/],
  ["removed_instrument", /mandate version/],
];

beforeEach(() => setPathname("/"));

describe("Who acts says where the owner ends a restriction (C-24)", () => {
  it.each(IN_STOP.map((c) => [c]))("%s names Stop and opens it", (code) => {
    banner(code);
    const where = whoActs();
    expect(where).toHaveTextContent(/in Stop/);
    expect(within(where).getByRole("button", { name: "Open Stop" })).toHaveAttribute("aria-haspopup", "dialog");
    expect(within(where).queryByRole("link")).toBeNull();
  });

  it.each(COMING)("%s names the place it will be and says it is not built, with no door", (code, place) => {
    banner(code);
    const where = whoActs();
    expect(where).toHaveTextContent(place);
    expect(where).toHaveTextContent(/next slice/);
    expect(within(where).queryByRole("link")).toBeNull();
    expect(within(where).queryByRole("button")).toBeNull();
  });

  it("names the broker for the 1× check, which ends outside the app", () => {
    banner("leverage_check_failed");
    expect(whoActs()).toHaveTextContent(/at the broker/);
    expect(within(whoActs()).queryByRole("button")).toBeNull();
  });

  it.each([["approver"], ["viewer"]] as const)("offers no Stop door to a role that cannot resume or release (%s)", (role) => {
    banner("owner_pause", role);
    expect(whoActs()).toHaveTextContent(/in Stop/);
    expect(within(whoActs()).queryByRole("button")).toBeNull();
  });

  it("opens Stop on this agent, where Resume still asks for the passkey before anything is sent", () => {
    setPathname(`/agents/${AGENT_IDS.swing}`);
    renderWithRuntime(
      <>
        <StopSheetHost />
        <ModeBanner mode="paused" restrictions={[{ code: "owner_pause", since: SINCE }]} />
      </>,
      "paused",
    );
    fireEvent.click(within(whoActs()).getByRole("button", { name: "Open Stop" }));
    const sheet = screen.getByRole("dialog");
    expect(within(sheet).getByRole("heading", { name: /This agent: Agent 2/ })).toBeInTheDocument();
    fireEvent.click(within(sheet).getByRole("button", { name: /^Resume Agent 2/ }));
    expect(stepUpDialog()).not.toBeNull();
    expect(screen.getByRole("button", { name: "Use passkey" })).toBeInTheDocument();
  });
});
