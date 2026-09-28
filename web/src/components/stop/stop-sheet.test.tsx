import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StopControl } from "@/components/shell/stop-control";
import type { Scenario } from "@/fixtures/types";
import { AGENT_IDS, SCENARIOS } from "@/fixtures/workspace";
import type { CommandKind } from "@/lib/mock-runtime";
import { PASSKEY_ANSWER_MS, type Passkey, mockPasskey } from "./step-up-dialog";
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

/**
 * Every command the sheet can send, where it is offered, and what the owner must see. `stepUp` is
 * the one line G3 shows, or null for the two pauses (PX-4 (b)). Keyed by command so a new command
 * does not compile until it has a row here.
 */
interface Case {
  name: string;
  path: string;
  scenario: Scenario;
  choice: RegExp;
  stepUp: string | null;
  recorded: string;
}

const CASES = {
  pause: [{ name: "Pause", path: `/agents/${AGENT_IDS.lmn}`, scenario: "normal", choice: /^Pause Agent 3/, stepUp: null, recorded: "Agent 3 is paused." }],
  pause_all: [
    { name: "Pause all agents", path: "/", scenario: "normal", choice: /^Pause all agents on this account/, stepUp: null, recorded: "Every agent on this account is paused." },
  ],
  resume: [
    {
      name: "Resume",
      path: `/agents/${AGENT_IDS.swing}`,
      scenario: "paused",
      choice: /^Resume Agent 2/,
      stepUp: "Resume Agent 2: it trades again within its mandate.",
      recorded: "Agent 2 is trading again.",
    },
  ],
  stop: [
    {
      name: "Stop",
      path: `/agents/${AGENT_IDS.lmn}`,
      scenario: "normal",
      choice: /^Stop Agent 3/,
      stepUp: "Stop Agent 3 for good. It holds nothing, so nothing is sold.",
      recorded: "Agent 3 is stopped.",
    },
  ],
  kill: [
    {
      name: "Kill switch, flat agent",
      path: `/agents/${AGENT_IDS.lmn}`,
      scenario: "normal",
      choice: /^Kill switch: cancel and stop/,
      stepUp: "Kill switch for Agent 3: cancel its orders, sell its positions, and end it.",
      recorded: "Agent 3: orders canceled, positions sold, agent stopped.",
    },
    {
      name: "Kill switch, agent holding positions",
      path: `/agents/${AGENT_IDS.btc}`,
      scenario: "normal",
      choice: /^Kill switch: close and stop/,
      stepUp: "Kill switch for Agent 1: cancel its orders, sell its positions, and end it.",
      recorded: "Agent 1: orders canceled, positions sold, agent stopped.",
    },
  ],
  release: [
    {
      name: "Stop and release positions to me",
      path: `/agents/${AGENT_IDS.btc}`,
      scenario: "normal",
      choice: /^Stop and release positions to me/,
      stepUp: "Stop Agent 1 and release 1 position to you, without protection.",
      recorded: "Agent 1 is stopped; its positions are yours and unprotected.",
    },
  ],
  stop_all: [
    {
      name: "Stop all agents on this account",
      path: "/",
      scenario: "normal",
      choice: /^Stop all agents on this account/,
      stepUp: "Stop all agents on this account: each agent's kill switch. Your own holdings stay.",
      recorded: "Every agent on this account is stopped by its kill switch.",
    },
  ],
  close_all: [
    {
      name: "Close everything on this account",
      path: "/",
      scenario: "normal",
      choice: /^Close everything on this account/,
      stepUp: "Close everything on this account: cancel every order and close every position, including ones no agent manages.",
      recorded: "Every order on this account is canceled, every position closed, and every agent stopped.",
    },
  ],
} satisfies Record<CommandKind, Case[]>;

const ALL: Case[] = Object.values(CASES).flat();
const STEP_UP = ALL.filter((c): c is Case & { stepUp: string } => c.stepUp !== null);
const NO_STEP_UP = ALL.filter((c) => c.stepUp === null);

const failingPasskey: Passkey = (_action, answer) => {
  const id = window.setTimeout(() => answer("failed"), PASSKEY_ANSWER_MS);
  return () => window.clearTimeout(id);
};

function stepUpDialog() {
  return screen.queryByRole("dialog", { name: "Confirm it is you" });
}

function entries(sheet: HTMLElement) {
  return log(sheet).querySelectorAll("[data-phase]");
}

/** The modes the sheet shows for every agent in view: they change only once the runtime records a command. */
function modes(sheet: HTMLElement) {
  return Array.from(sheet.querySelectorAll("[data-slot=mode-badge]"), (b) => b.getAttribute("data-mode"));
}

function start(c: Case, options?: { passkey?: Passkey }) {
  setPathname(c.path);
  renderWithRuntime(<StopControl />, c.scenario, options);
  const sheet = openSheet();
  const before = modes(sheet);
  fireEvent.click(within(sheet).getByRole("button", { name: c.choice }));
  return { sheet, before };
}

function press(name: "Use passkey" | "Cancel") {
  fireEvent.click(within(stepUpDialog()!).getByRole("button", { name }));
}

/** Nothing reached the runtime: no entry now or later, and every agent keeps its mode. */
function expectNothingSent(sheet: HTMLElement, before: Array<string | null>) {
  expect(entries(sheet)).toHaveLength(0);
  act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));
  expect(entries(sheet)).toHaveLength(0);
  expect(modes(sheet)).toEqual(before);
}

/** Rule 5: "sent" first, the recorded state only once the runtime records it, and exactly one command. */
function expectSentThenRecorded(sheet: HTMLElement, before: Array<string | null>, c: Case) {
  expect(entries(sheet)).toHaveLength(1);
  expect(entries(sheet)[0]).toHaveAttribute("data-phase", "sent");
  expect(log(sheet)).toHaveTextContent("Sent; waiting for the runtime to record it.");
  expect(log(sheet)).not.toHaveTextContent("Recorded");
  expect(log(sheet)).not.toHaveTextContent(c.recorded);
  expect(modes(sheet)).toEqual(before);

  act(() => vi.advanceTimersByTime(RECORD_AFTER_MS - 1));
  expect(entries(sheet)[0]).toHaveAttribute("data-phase", "sent");
  expect(modes(sheet)).toEqual(before);

  act(() => vi.advanceTimersByTime(1));
  expect(entries(sheet)).toHaveLength(1);
  expect(entries(sheet)[0]).toHaveAttribute("data-phase", "recorded");
  expect(log(sheet)).toHaveTextContent(new RegExp(`Recorded at \\d{2}:\\d{2}:\\d{2}\\. ${literal(c.recorded)}`));
  expect(log(sheet)).not.toHaveTextContent("Sent; waiting");
  expect(modes(sheet)).not.toEqual(before);
}

function literal(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

describe("Pause needs no step-up (PX-4 (b))", () => {
  it.each(NO_STEP_UP.map((c) => [c.name, c] as const))("%s is sent at once, then shown recorded only after the runtime records it", (_, c) => {
    const { sheet, before } = start(c);
    expect(stepUpDialog()).toBeNull();
    expectSentThenRecorded(sheet, before, c);
    expect(stepUpDialog()).toBeNull();
  });
});

describe("every other command needs a passkey first (PX-4, PX-12, DEC-136)", () => {
  it.each(STEP_UP.map((c) => [c.name, c] as const))("%s opens the passkey check naming the action, and sends nothing while it waits", (_, c) => {
    const { sheet, before } = start(c);
    const dialog = stepUpDialog();
    expect(dialog).not.toBeNull();
    expect(dialog!.querySelector("[data-slot=step-up-action]")).toHaveTextContent(c.stepUp);
    expectNothingSent(sheet, before);
    expect(stepUpDialog()).not.toBeNull();
  });

  it.each(STEP_UP.map((c) => [c.name, c] as const))("%s asks the passkey for this one action", (_, c) => {
    const passkey = vi.fn<Passkey>(mockPasskey);
    start(c, { passkey });
    expect(passkey).not.toHaveBeenCalled();
    press("Use passkey");
    expect(passkey).toHaveBeenCalledTimes(1);
    expect(passkey).toHaveBeenCalledWith(c.stepUp, expect.any(Function));
  });
});

interface Ending {
  ending: string;
  notice: string;
  passkey?: Passkey;
  run: () => void;
}

const ENDINGS: Ending[] = [
  {
    ending: "Cancel",
    notice: "Passkey check canceled. Nothing was sent.",
    run: () => press("Cancel"),
  },
  {
    ending: "Escape",
    notice: "Passkey check canceled. Nothing was sent.",
    run: () => fireEvent.keyDown(stepUpDialog()!, { key: "Escape" }),
  },
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
    ending: "a failed passkey check",
    notice: "Passkey check failed. Nothing was sent.",
    passkey: failingPasskey,
    run: () => {
      press("Use passkey");
      act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    },
  },
];

describe("a cancelled or failed passkey check never becomes an action (G3, P4)", () => {
  it.each(STEP_UP.flatMap((c) => ENDINGS.map((e) => [c.name, e.ending, c, e] as const)))("%s, ended by %s, sends nothing and says so", (_, __, c, e) => {
    const { sheet, before } = start(c, { passkey: e.passkey });
    e.run();
    expect(stepUpDialog()).toBeNull();
    expect(screen.getByRole("dialog")).toBe(sheet);
    expect(log(sheet)).toHaveTextContent(e.notice);
    expectNothingSent(sheet, before);
  });
});

describe("after a verified passkey, the command is sent and never shown as done early (rule 5)", () => {
  it.each(STEP_UP.map((c) => [c.name, c] as const))("%s shows sent, then recorded only once the runtime records it", (_, c) => {
    const { sheet, before } = start(c);
    press("Use passkey");
    expect(stepUpDialog()).toHaveTextContent("Waiting for your passkey…");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS - 1));
    expect(entries(sheet)).toHaveLength(0);
    act(() => vi.advanceTimersByTime(1));
    expect(stepUpDialog()).toBeNull();
    expectSentThenRecorded(sheet, before, c);
  });

  it.each(STEP_UP.map((c) => [c.name, c] as const))("%s is sent once when Use passkey is pressed twice", (_, c) => {
    const { sheet } = start(c);
    press("Use passkey");
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS + RECORD_AFTER_MS));
    expect(entries(sheet)).toHaveLength(1);
  });
});

describe("Stop sheet when the deployment is unreachable", () => {
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
