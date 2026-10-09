import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentOwl } from "@/components/domain/owl";
import { StopControl } from "@/components/shell/stop-control";
import { StopSheet } from "@/components/stop/stop-sheet";
import type { AgentMode, Scenario, Workspace } from "@/fixtures/types";
import { AGENT_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { heldPasskey, watchConsole } from "@/test/passkey";
import { ENDINGS, RuntimeCommands, entries, expectNothingSent, expectSentThenRecorded, log, modes, press, recordedCount, stepUpDialog } from "@/test/step-up";
import { type RecordKind, type SheetKind, recordHref } from "./commands";
import { PASSKEY_ANSWER_MS, type Passkey, type PasskeyResult, mockPasskey } from "./step-up-dialog";

function openSheet() {
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  return screen.getByRole("dialog");
}

/** Buttons act in the sheet; links open a record screen. Both are choices, in the order shown. */
function choices(section: HTMLElement) {
  return Array.from(section.querySelectorAll<HTMLElement>("[data-tone]"));
}

function choiceTitles(section: HTMLElement) {
  return choices(section).map((b) => b.querySelector("span")?.textContent ?? "");
}

function section(sheet: HTMLElement, heading: RegExp) {
  return within(sheet).getByRole("heading", { name: heading }).closest("section")!;
}

/** An owl's eyes say its mode and nothing else (DEC-217); written out here, not read from `moodFor`. */
const OWL_MOODS: Array<[AgentMode, string]> = [
  ["normal", "awake"],
  ["exits_only", "focused"],
  ["paused", "asleep"],
  ["stopped", "stopped"],
];

/** The owl's body, feathers included: the pixels its ID draws, apart from the eyes that follow the pointer. */
function bodyPixels(owl: Element) {
  return Array.from(owl.querySelectorAll(".owl-body > rect")).map((r) => ["x", "y", "width", "height", "fill"].map((k) => r.getAttribute(k)).join(","));
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
    expect(within(agent).getByRole("link", { name: /Kill switch: close and stop/ })).toHaveAttribute("href", recordHref("kill", AGENT_IDS.swing));
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
    expect(within(sheet).getByRole("link", { name: /Stop all agents on this account/ })).toHaveAttribute("href");
  });

  it("focuses the sheet and the passkey dialog themselves, never an action", () => {
    setPathname(`/agents/${AGENT_IDS.lmn}`);
    renderWithRuntime(<StopControl />);
    const sheet = openSheet();
    expect(document.activeElement).toBe(sheet);
    fireEvent.click(within(sheet).getByRole("button", { name: /^Stop Agent 3/ }));
    expect(document.activeElement).toBe(screen.getByRole("dialog", { name: "Confirm it is you" }));
  });

  it.each(OWL_MOODS)("puts each agent's own owl at the start of its row under One agent, in its %s mode, hidden from the row's name (C-10)", (mode, mood) => {
    const workspace = (ws: Workspace) => ({ ...ws, agents: ws.agents.map((a) => ({ ...a, mode })) });
    renderWithRuntime(<StopControl />, "normal", { workspace });
    const agents = workspace(buildWorkspace("normal")).agents;
    const rows = section(openSheet(), /One agent/);
    const triggers = within(rows).getAllByRole("button");
    expect(triggers).toHaveLength(agents.length);
    agents.forEach((agent, i) => {
      const trigger = triggers[i];
      const owl = trigger.querySelector("svg[data-slot=owl]");
      expect(owl, `${agent.label}'s owl`).not.toBeNull();
      expect(trigger.firstElementChild, `${agent.label}'s owl starts the row`).toBe(owl);
      expect(trigger.querySelectorAll("svg[data-slot=owl]")).toHaveLength(1);
      expect(owl).toHaveAttribute("aria-hidden", "true");
      expect(owl).toHaveAttribute("data-mood", mood);
      expect(owl).toHaveClass("size-8");
      const alone = render(<AgentOwl agent={agent} className="size-8" />).container.querySelector("svg[data-slot=owl]")!;
      expect(bodyPixels(owl!), `${agent.label}'s owl wears its own feathers`).toEqual(bodyPixels(alone));
      const badge = trigger.querySelector("[data-slot=mode-badge]")!;
      expect(trigger).toHaveAccessibleName(`${agent.label}${agent.mandate.name}${badge.textContent}`);
    });
  });

  it("keeps the expanded row's choices, their names and order, free of the owl (C-10)", () => {
    renderWithRuntime(<StopControl />);
    const rows = section(openSheet(), /One agent/);
    fireEvent.click(within(rows).getByRole("button", { name: /^Agent 1/ }));
    expect(choiceTitles(rows)).toEqual(["Pause Agent 1", "Kill switch: close and stop", "Stop and release positions to me"]);
    expect(within(rows).getByRole("button", { name: /^Pause Agent 1/ })).toBeInTheDocument();
    expect(within(rows).getByRole("link", { name: /^Kill switch: close and stop/ })).toHaveAttribute("href", recordHref("kill", AGENT_IDS.btc));
    for (const choice of choices(rows)) expect(choice.querySelector("svg[data-slot=owl]"), choice.textContent ?? "").toBeNull();
  });

  it("keeps account-wide choices while agent data is loading", () => {
    renderWithRuntime(<StopControl />, "loading");
    const sheet = openSheet();
    expect(sheet).toHaveTextContent("Agent details are still loading");
    expect(within(sheet).getByRole("button", { name: /Pause all agents/ })).not.toBeDisabled();
  });
});


/** D10 and D11 are record screens, so pages: the sheet is the chooser and links to them (brief §4.1). */
const LINKS: Array<{ kind: RecordKind; path: string; choice: RegExp; target: () => string }> = [
  { kind: "kill", path: `/agents/${AGENT_IDS.lmn}`, choice: /^Kill switch: cancel and stop/, target: () => AGENT_IDS.lmn },
  { kind: "kill", path: `/agents/${AGENT_IDS.btc}`, choice: /^Kill switch: close and stop/, target: () => AGENT_IDS.btc },
  { kind: "release", path: `/agents/${AGENT_IDS.btc}`, choice: /^Stop and release positions to me/, target: () => AGENT_IDS.btc },
  { kind: "stop_all", path: "/", choice: /^Stop all agents on this account/, target: () => buildWorkspace("normal").connection.connection_id },
  { kind: "close_all", path: "/", choice: /^Close everything on this account/, target: () => buildWorkspace("normal").connection.connection_id },
];

describe("the kill switch and release open their record screens", () => {
  it.each(LINKS.map((l) => [l.choice.source, l] as const))("%s links to its page, sends nothing, and closes the sheet", (_, l) => {
    setPathname(l.path);
    renderWithRuntime(
      <>
        <StopControl />
        <RuntimeCommands />
      </>,
    );
    const sheet = openSheet();
    const link = within(sheet).getByRole("link", { name: l.choice });
    expect(link).toHaveAttribute("href", recordHref(l.kind, l.target()));
    expect(link).toHaveTextContent("Opens the full list to confirm with your passkey.");
    fireEvent.click(link);
    expect(stepUpDialog()).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));
    expect(recordedCount()).toBe(0);
  });

  it("uses opaque IDs in every record link", () => {
    setPathname(`/agents/${AGENT_IDS.btc}`);
    renderWithRuntime(<StopControl />);
    const hrefs = Array.from(openSheet().querySelectorAll("a[data-tone]"), (a) => a.getAttribute("href") ?? "");
    expect(hrefs).toHaveLength(4);
    for (const href of hrefs) expect(href).toMatch(/^\/(agents\/agt|connections\/con)_[0-9A-HJKMNP-TV-Z]{26}\/[a-z-]+$/);
  });
});

/**
 * Every command the sheet itself sends, where it is offered, and what the owner must see. `stepUp`
 * is the one line G3 shows, or null for the two pauses (PX-4 (b)). Keyed by command so a new
 * command does not compile until it has a row here or in the record screens' table.
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
} satisfies Record<SheetKind, Case[]>;

const ALL: Case[] = Object.values(CASES).flat();
const STEP_UP = ALL.filter((c): c is Case & { stepUp: string } => c.stepUp !== null);
const NO_STEP_UP = ALL.filter((c) => c.stepUp === null);

function start(c: Case, options?: { passkey?: Passkey }, ui = <StopControl />) {
  setPathname(c.path);
  const view = renderWithRuntime(ui, c.scenario, options);
  const sheet = openSheet();
  const before = modes(sheet);
  fireEvent.click(within(sheet).getByRole("button", { name: c.choice }));
  return { sheet, before, view };
}

describe("Pause needs no step-up (PX-4 (b))", () => {
  it.each(NO_STEP_UP.map((c) => [c.name, c] as const))("%s is sent at once, then shown recorded only after the runtime records it", (_, c) => {
    const { sheet, before } = start(c);
    expect(stepUpDialog()).toBeNull();
    expectSentThenRecorded(sheet, before, c.recorded);
    expect(stepUpDialog()).toBeNull();
  });
});

describe("every other command in the sheet needs a passkey first (PX-4, DEC-136)", () => {
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
    expectSentThenRecorded(sheet, before, c.recorded);
  });

  it.each(STEP_UP.map((c) => [c.name, c] as const))("%s is sent once when Use passkey is pressed twice", (_, c) => {
    const { sheet } = start(c);
    press("Use passkey");
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS + RECORD_AFTER_MS));
    expect(entries(sheet)).toHaveLength(1);
  });
});

function Tree({ stop }: { stop: boolean }) {
  return (
    <>
      {stop ? <StopControl /> : null}
      <RuntimeCommands />
    </>
  );
}

describe("unmounting mid-passkey abandons the request, and a late answer never becomes an action (P4)", () => {
  let console_: ReturnType<typeof watchConsole>;
  beforeEach(() => {
    console_ = watchConsole();
  });
  afterEach(() => console_.restore());

  const RESULTS: PasskeyResult[] = ["verified", "failed"];
  const CASES_BY_RESULT = STEP_UP.flatMap((c) => RESULTS.map((r) => [c.name, r, c] as const));

  it("counts a command the runtime receives, so the unmount tests below can see one", () => {
    const [c] = STEP_UP;
    start(c, undefined, <Tree stop />);
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    expect(recordedCount()).toBe(1);
  });

  it.each(CASES_BY_RESULT)("%s: removing the Stop control while the passkey is asked sends nothing, even when it later answers %s", (_, result, c) => {
    const { passkey, release } = heldPasskey(result);
    const { view } = start(c, { passkey }, <Tree stop />);
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS - 1));

    view.rerender(<Tree stop={false} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));

    expect(recordedCount()).toBe(0);
    expect(release).toHaveBeenCalledTimes(1);
    expect(console_.calls()).toEqual([]);
  });

  it.each(CASES_BY_RESULT)("%s: closing the Stop sheet while the passkey is asked sends nothing, even when it later answers %s, and it reopens without the old check", (_, result, c) => {
    const { passkey, release } = heldPasskey(result);
    const agentId = c.path.startsWith("/agents/") ? c.path.slice("/agents/".length) : null;
    const ui = (open: boolean) => (
      <>
        <StopSheet open={open} onOpenChange={() => {}} agentId={agentId} />
        <RuntimeCommands />
      </>
    );
    setPathname(c.path);
    const view = renderWithRuntime(ui(true), c.scenario, { passkey });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: c.choice }));
    press("Use passkey");

    view.rerender(ui(false));
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));
    expect(recordedCount()).toBe(0);
    expect(release).toHaveBeenCalledTimes(1);

    view.rerender(ui(true));
    expect(stepUpDialog()).toBeNull();
    expect(screen.getByRole("dialog")).not.toHaveTextContent("Nothing was sent");
    expect(console_.calls()).toEqual([]);
  });

  it.each(CASES_BY_RESULT)("%s: unmounting the whole screen while the passkey is asked abandons it, with no warning when it later answers %s", (_, result, c) => {
    const { passkey, release } = heldPasskey(result);
    const { view } = start(c, { passkey });
    press("Use passkey");

    view.unmount();
    expect(release).toHaveBeenCalledTimes(1);
    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));

    expect(console_.calls()).toEqual([]);
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
