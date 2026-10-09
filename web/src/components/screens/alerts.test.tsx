import type { ReactElement } from "react";
import { cleanup, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import type { Scenario, Workspace } from "@/fixtures/types";
import { SCENARIOS } from "@/fixtures/workspace";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AlertsScreen } from "./account-screens";
import { DashboardScreen } from "./dashboard";

/**
 * Critique C-13: on a calm day the Alerts screen opens with Home's all-clear line, and only then. Calm
 * means every feed answers and Home itself would say all clear, so the two screens never disagree.
 */

const ALL_CLEAR = /^All clear\. Nothing needs you\.$/;

/** The fixture's waiting requests answered, so only feeds and agents' conditions are left to decide. */
const noRequests = (ws: Workspace): Workspace => ({ ...ws, approvals: ws.approvals.filter((a) => a.status !== "delivered") });

function render(path: string, ui: ReactElement, scenario: Scenario, workspace?: (ws: Workspace) => Workspace) {
  setPathname(path);
  renderWithRuntime(<AppShell>{ui}</AppShell>, scenario, { workspace });
  return screen.getByRole("main");
}

const alertsClear = (scenario: Scenario, workspace?: (ws: Workspace) => Workspace) =>
  render("/alerts", <AlertsScreen />, scenario, workspace).querySelector<HTMLElement>("[data-slot=all-clear]");

const homeClear = (scenario: Scenario, workspace?: (ws: Workspace) => Workspace) =>
  render("/", <DashboardScreen />, scenario, workspace).querySelector<HTMLElement>("[data-slot=all-clear]");

/** Every feed answering, read straight from the fixture rather than through the screen's own predicate. */
const feedsAnswer = (ws: Workspace) => ws.status === "ready" && [ws.health.market_data, ws.health.broker, ws.health.deployment, ws.health.relay].every((f) => f.state === "ok");

beforeEach(() => setPathname("/alerts"));

describe("Alerts says all clear on a calm day (C-13)", () => {
  it("opens with Home's all-clear line when every feed answers and nothing needs the owner", () => {
    const main = render("/alerts", <AlertsScreen />, "normal", noRequests);
    const clear = main.querySelector<HTMLElement>("[data-slot=all-clear]");
    expect(clear, "the all-clear line is on Alerts").not.toBeNull();
    expect(clear).toHaveTextContent(ALL_CLEAR);
    const firstHeading = screen.getByRole("heading", { name: "Data and deployment" });
    expect(clear!.compareDocumentPosition(firstHeading) & Node.DOCUMENT_POSITION_FOLLOWING, "the page opens with it, above the feeds").toBeTruthy();
    cleanup();
    expect(homeClear("normal", noRequests), "Home says the same on the same workspace").toHaveTextContent(ALL_CLEAR);
  });

  it.each<[string, Scenario]>([
    ["a feed is stale or down", "stale"],
    ["the deployment is unreachable", "unreachable"],
  ])("does not say all clear when %s", (_why, scenario) => {
    expect(alertsClear(scenario, noRequests)).toBeNull();
  });

  it("does not say all clear when a feed is stale but no agent is restricted", () => {
    const staleFeedOnly = (ws: Workspace): Workspace => ({ ...noRequests(ws), health: { ...ws.health, broker: { ...ws.health.broker, state: "stale" } } });
    expect(alertsClear("normal", staleFeedOnly)).toBeNull();
  });

  it("does not say all clear when a feed is down but no agent is restricted", () => {
    const relayDown = (ws: Workspace): Workspace => ({ ...noRequests(ws), health: { ...ws.health, relay: { ...ws.health.relay, state: "down" } } });
    expect(alertsClear("normal", relayDown)).toBeNull();
  });

  it.each<[string, Scenario]>([
    ["an agent is paused", "paused"],
    ["an agent is selling only after a drawdown", "drawdown"],
    ["an agent is held for reconciliation", "reconciliation"],
    ["an order's state is unknown", "unknown-order"],
  ])("does not say all clear when %s", (_why, scenario) => {
    expect(alertsClear(scenario, noRequests)).toBeNull();
  });

  it.each<[string, Scenario]>([
    ["a request waits for the owner", "normal"],
    ["several requests wait", "approvals"],
  ])("does not say all clear when %s, since Home would not", (_why, scenario) => {
    expect(alertsClear(scenario)).toBeNull();
  });

  /** Scenarios where Home shows its Needs you section: the empty, loading and unreachable ones do not. */
  const WITH_AGENTS = SCENARIOS.map((s) => s.id).filter((id) => !["empty", "loading", "unreachable"].includes(id));

  it.each(WITH_AGENTS.flatMap((id) => [[id, "as recorded"] as const, [id, "with no request waiting"] as const]))(
    "%s, %s: Alerts says all clear exactly when every feed answers and Home says all clear",
    (scenario, variant) => {
      const shape = variant === "as recorded" ? (ws: Workspace) => ws : noRequests;
      let ws: Workspace | undefined;
      const onAlerts = alertsClear(scenario, (built) => (ws = shape(built))) !== null;
      cleanup();
      const onHome = homeClear(scenario, shape) !== null;
      expect(onAlerts).toBe(feedsAnswer(ws!) && onHome);
    },
  );
});
