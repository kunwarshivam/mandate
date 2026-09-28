import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StopControl } from "@/components/shell/stop-control";
import { AGENT_IDS, SCENARIOS } from "@/fixtures/workspace";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";

function openSheet() {
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  return screen.getByRole("dialog");
}

function choiceTitles(section: HTMLElement) {
  return within(section)
    .getAllByRole("button")
    .map((b) => b.querySelector("span")?.textContent ?? "");
}

function section(sheet: HTMLElement, heading: RegExp) {
  return within(sheet).getByRole("heading", { name: heading }).closest("section")!;
}

function log(sheet: HTMLElement) {
  return within(sheet).getByRole("status");
}

beforeEach(() => {
  vi.useFakeTimers();
  setPathname("/");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Stop sheet choices", () => {
  it("offers Pause first, then Stop only when the agent is flat", () => {
    setPathname(`/agents/${AGENT_IDS.lmn}`);
    renderWithRuntime(<StopControl />);
    const titles = choiceTitles(section(openSheet(), /This agent: Agent 3/));
    expect(titles).toEqual(["Pause Agent 3", "Stop Agent 3", "Kill switch: cancel and stop"]);
  });

  it("offers the kill switch or release, not Stop, when the agent holds positions", () => {
    setPathname(`/agents/${AGENT_IDS.btc}`);
    renderWithRuntime(<StopControl />);
    const agent = section(openSheet(), /This agent: Agent 1/);
    expect(choiceTitles(agent)).toEqual(["Pause Agent 1", "Kill switch: close and stop", "Stop and release positions to me"]);
    expect(within(agent).getByText("without protection")).toBeInTheDocument();
  });

  it("orders the account-wide choices pause, stop all, then close everything, and lists what closing does", () => {
    renderWithRuntime(<StopControl />);
    const account = section(openSheet(), /Everything on this account/);
    expect(choiceTitles(account)).toEqual(["Pause all agents on this account", "Stop all agents on this account", "Close everything on this account"]);
    expect(account).toHaveTextContent("including ones Owlhead did not place");
    expect(account).toHaveTextContent("including your own 20 ABC");
  });

  it("says an unknown order holds exits while the kill switch still works", () => {
    setPathname(`/agents/${AGENT_IDS.swing}`);
    renderWithRuntime(<StopControl />, "unknown-order");
    const agent = section(openSheet(), /This agent: Agent 2/);
    expect(agent).toHaveTextContent("An order has an unknown state");
    expect(within(agent).getByRole("button", { name: /Kill switch: close and stop/ })).not.toBeDisabled();
  });

  it("explains a reconciliation hold without asking anything of the owner", () => {
    setPathname(`/agents/${AGENT_IDS.swing}`);
    renderWithRuntime(<StopControl />, "reconciliation");
    expect(section(openSheet(), /This agent: Agent 2/)).toHaveTextContent("Checking with the broker");
  });

  it.each(SCENARIOS.map((s) => s.id))("keeps the Stop control and every choice enabled in the %s scenario", (scenario) => {
    renderWithRuntime(<StopControl />, scenario);
    const control = screen.getByRole("button", { name: "Stop" });
    expect(isDisabled(control)).toBe(false);
    const sheet = openSheet();
    for (const button of within(sheet).getAllByRole("button")) expect(isDisabled(button), button.textContent ?? "").toBe(false);
    expect(within(sheet).getByRole("button", { name: /Stop all agents on this account/ })).toBeInTheDocument();
  });

  it("focuses the sheet and the passkey dialog themselves, never an action", () => {
    setPathname(`/agents/${AGENT_IDS.lmn}`);
    renderWithRuntime(<StopControl />);
    const sheet = openSheet();
    expect(document.activeElement).toBe(sheet);
    fireEvent.click(within(sheet).getByRole("button", { name: /^Stop Agent 3/ }));
    expect(document.activeElement).toBe(screen.getByRole("dialog", { name: "Confirm it is you" }));
  });

  it("keeps account-wide choices while agent data is loading", () => {
    renderWithRuntime(<StopControl />, "loading");
    const sheet = openSheet();
    expect(sheet).toHaveTextContent("Agent details are still loading");
    expect(within(sheet).getByRole("button", { name: /Pause all agents/ })).not.toBeDisabled();
  });
});

describe("Stop sheet commands are never optimistic", () => {
  it("sends Pause without step-up and shows it recorded only after the runtime records it", () => {
    setPathname(`/agents/${AGENT_IDS.lmn}`);
    renderWithRuntime(<StopControl />);
    const sheet = openSheet();
    fireEvent.click(within(sheet).getByRole("button", { name: /Pause Agent 3/ }));
    expect(screen.queryByText("Confirm it is you")).not.toBeInTheDocument();
    expect(log(sheet)).toHaveTextContent("Sent; waiting for the runtime to record it.");
    expect(log(sheet)).not.toHaveTextContent(/Recorded|Paused/);

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS - 1));
    expect(log(sheet)).not.toHaveTextContent("Recorded");
    act(() => vi.advanceTimersByTime(1));
    expect(log(sheet)).toHaveTextContent(/Recorded at \d{2}:\d{2}:\d{2}\. Agent 3 is paused/);
  });

  it("asks for a passkey before Stop, and a canceled check sends nothing", () => {
    setPathname(`/agents/${AGENT_IDS.lmn}`);
    renderWithRuntime(<StopControl />);
    const sheet = openSheet();
    fireEvent.click(within(sheet).getByRole("button", { name: /^Stop Agent 3/ }));
    const stepUp = screen.getByRole("dialog", { name: "Confirm it is you" });
    fireEvent.click(within(stepUp).getByRole("button", { name: "Cancel" }));
    expect(log(sheet)).toHaveTextContent("Passkey check canceled. Nothing was sent.");
    expect(log(sheet).querySelector("[data-phase]")).toBeNull();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(log(sheet).querySelector("[data-phase]")).toBeNull();
  });

  it("sends Stop only after the passkey answers, then waits for the record", () => {
    setPathname(`/agents/${AGENT_IDS.lmn}`);
    renderWithRuntime(<StopControl />);
    const sheet = openSheet();
    fireEvent.click(within(sheet).getByRole("button", { name: /^Stop Agent 3/ }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Confirm it is you" })).getByRole("button", { name: "Use passkey" }));
    expect(screen.getByText("Waiting for your passkey…")).toBeInTheDocument();
    expect(log(sheet).querySelector("[data-phase]")).toBeNull();

    act(() => vi.advanceTimersByTime(900));
    expect(log(sheet).querySelector("[data-phase]")).toHaveAttribute("data-phase", "sent");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(log(sheet).querySelector("[data-phase]")).toHaveAttribute("data-phase", "recorded");
    expect(log(sheet)).toHaveTextContent("Agent 3 is stopped");
  });

  it("when the deployment is unreachable, explains the broker route and reports the request as not delivered", () => {
    renderWithRuntime(<StopControl />, "unreachable");
    const sheet = openSheet();
    expect(sheet).toHaveTextContent("Cannot reach your deployment");
    expect(sheet).toHaveTextContent("sign in to your Alpaca paper dashboard");
    fireEvent.click(within(sheet).getByRole("button", { name: /Pause all agents/ }));
    expect(log(sheet)).toHaveTextContent("Sent; waiting for the runtime to record it.");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(log(sheet)).toHaveTextContent("Not delivered");
    expect(log(sheet)).not.toHaveTextContent("Recorded");
  });
});
