import { useEffect } from "react";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as killPage from "@/app/(app)/agents/[agentId]/kill-switch/page";
import * as releasePage from "@/app/(app)/agents/[agentId]/release/page";
import * as closeAllPage from "@/app/(app)/connections/[connectionId]/close-all/page";
import * as stopAllPage from "@/app/(app)/connections/[connectionId]/stop-all/page";
import type { Scenario } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { MODE_LABEL } from "@/lib/labels";
import { type CommandKind, useRuntime } from "@/lib/mock-runtime";
import { RECORD_AFTER_MS, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { heldPasskey, watchConsole } from "@/test/passkey";
import {
  ENDINGS,
  RuntimeCommands,
  entries,
  expectNothingSent,
  expectSentThenRecorded,
  log,
  modes,
  press,
  recordRegion,
  recordedCount,
  runtimeCommands,
  stepUpDialog,
} from "@/test/step-up";
import { type RecordKind, recordHref } from "./commands";
import { buildRecord, modeLine, recordLines } from "./record";
import { StopRecordScreen } from "./record-screen";
import { PASSKEY_ANSWER_MS, type Passkey, type PasskeyResult, mockPasskey } from "./step-up-dialog";

const CONNECTION = buildWorkspace("normal").connection.connection_id;

/**
 * Every command confirmed on a record screen (D10, D11). Keyed by command so a new record kind does
 * not compile until it has a row here; the Stop sheet's own commands have their table beside it.
 */
interface Case {
  name: string;
  target: string;
  scenario: Scenario;
  activate: RegExp;
  stepUp: string;
  recorded: string;
}

const CASES = {
  kill: [
    {
      name: "Kill switch, flat agent",
      target: AGENT_IDS.lmn,
      scenario: "normal",
      activate: /^Activate the kill switch/,
      stepUp: "Kill switch for Agent 3: cancel its orders, sell its positions, and end it.",
      recorded: "Agent 3: orders canceled, positions sold, agent stopped.",
    },
    {
      name: "Kill switch, agent holding positions",
      target: AGENT_IDS.btc,
      scenario: "normal",
      activate: /^Activate the kill switch/,
      stepUp: "Kill switch for Agent 1: cancel its orders, sell its positions, and end it.",
      recorded: "Agent 1: orders canceled, positions sold, agent stopped.",
    },
  ],
  release: [
    {
      name: "Stop and release positions to me",
      target: AGENT_IDS.btc,
      scenario: "normal",
      activate: /^Stop and release to me/,
      stepUp: "Stop Agent 1 and release 1 position to you, without protection.",
      recorded: "Agent 1 is stopped; its positions are yours and unprotected.",
    },
  ],
  stop_all: [
    {
      name: "Stop all agents on this account",
      target: CONNECTION,
      scenario: "normal",
      activate: /^Stop all agents on this account/,
      stepUp: "Stop all agents on this account: each agent's kill switch. Your own holdings stay.",
      recorded: "Every agent on this account is stopped by its kill switch.",
    },
  ],
  close_all: [
    {
      name: "Close everything on this account",
      target: CONNECTION,
      scenario: "normal",
      activate: /^Close everything on this account/,
      stepUp: "Close everything on this account: cancel every order and close every position, including ones no agent manages.",
      recorded: "Every order on this account is canceled, every position closed, and every agent stopped.",
    },
  ],
} satisfies Record<RecordKind, Case[]>;

type KindCase = Case & { kind: RecordKind };
const ALL: KindCase[] = (Object.entries(CASES) as Array<[RecordKind, Case[]]>).flatMap(([kind, cases]) => cases.map((c) => ({ ...c, kind })));
const named = ALL.map((c) => [c.name, c] as const);

function page() {
  return document.querySelector<HTMLElement>("[data-slot=record-screen]")!;
}

/** Sends a command the way another screen would, so the workspace changes under the record. */
const elsewhere: { send?: (kind: CommandKind, agentId: string | null) => void } = {};

function Elsewhere() {
  const { send } = useRuntime();
  useEffect(() => {
    elsewhere.send = send;
  }, [send]);
  return null;
}

function Tree({ c, shown = true }: { c: KindCase; shown?: boolean }) {
  return (
    <>
      {shown ? <StopRecordScreen kind={c.kind} targetId={c.target} /> : null}
      <RuntimeCommands />
      <Elsewhere />
    </>
  );
}

/** Each mode badge in a region, as the agent's label beside the badge's text. */
function badges(root: HTMLElement) {
  return Array.from(root.querySelectorAll("[data-slot=mode-badge]"), (b) => ({
    agent: b.closest("li")!.firstChild!.textContent!,
    badge: b.textContent!.trim(),
  }));
}

/** The agent a case's record shows first: the one it acts on, or the first on the account. */
function agentOf(c: KindCase): string {
  return c.kind === "kill" || c.kind === "release" ? c.target : AGENT_IDS.btc;
}

function start(c: KindCase, options?: { passkey?: Passkey; role?: "owner" | "operator" }) {
  setPathname(recordHref(c.kind, c.target));
  const view = renderWithRuntime(<Tree c={c} />, c.scenario, options);
  const before = modes(page());
  fireEvent.click(within(page()).getByRole("button", { name: c.activate }));
  return { before, view };
}

beforeEach(() => {
  vi.useFakeTimers();
  setPathname("/");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("record screens are pages (brief §4.1, D10, D11)", () => {
  const PAGES = [
    { kind: "kill", mod: killPage, param: "agentId", good: AGENT_IDS.btc, title: "Kill switch (paper)" },
    { kind: "release", mod: releasePage, param: "agentId", good: AGENT_IDS.btc, title: "Stop and release positions (paper)" },
    { kind: "stop_all", mod: stopAllPage, param: "connectionId", good: CONNECTION, title: "Stop all agents (paper)" },
    { kind: "close_all", mod: closeAllPage, param: "connectionId", good: CONNECTION, title: "Close everything (paper)" },
  ] as const;

  it.each(PAGES.map((p) => [p.kind, p] as const))("%s takes only an opaque ID in its route", async (_, p) => {
    const at = (id: string) => p.mod.default({ params: Promise.resolve({ [p.param]: id } as { agentId: string } & { connectionId: string }) });
    await expect(at(p.good)).resolves.toBeDefined();
    for (const bad of ["1", "agent-1", "conn_alpaca_paper_01", "BTC", `${p.good}x`]) await expect(at(bad), bad).rejects.toThrow("NEXT_NOT_FOUND");
  });

  it.each(PAGES.map((p) => [p.kind, p] as const))("%s names its environment in the document title, and no agent or instrument", async (_, p) => {
    expect((await p.mod.generateMetadata()).title).toBe(p.title);
  });

  it.each(named)("%s shows the environment in the page title", (_, c) => {
    renderWithRuntime(<StopRecordScreen kind={c.kind} targetId={c.target} />, c.scenario);
    const title = within(page()).getByRole("heading", { level: 1 });
    expect(within(title).getByText("PAPER")).toBeInTheDocument();
  });

  it.each(named)("%s shows every line of its record expanded, with nothing collapsed or hidden", (_, c) => {
    renderWithRuntime(<StopRecordScreen kind={c.kind} targetId={c.target} />, c.scenario);
    const record = buildRecord(c.kind, buildWorkspace(c.scenario), c.target)!;
    const modeLines = record.modes.map(modeLine);
    for (const line of recordLines(record).filter((l) => !modeLines.includes(l))) expect(recordRegion().textContent, line).toContain(line);
    expect(badges(recordRegion())).toEqual(record.modes.map((m) => ({ agent: m.label, badge: MODE_LABEL[m.mode] })));
    expect(page().querySelector("[aria-expanded], details, [hidden], [data-panel-open], [data-closed]")).toBeNull();
    expect(page().querySelectorAll("[data-list]").length).toBe(record.lists.length);
  });

  it("lists what the agent kill switch cancels and sells, and what it leaves alone", () => {
    renderWithRuntime(<StopRecordScreen kind="kill" targetId={AGENT_IDS.btc} />);
    expect(page()).toHaveTextContent("Cancels only this agent's orders, sells only its positions, leaves other agents and your own holdings untouched");
    const sells = page().querySelector("[data-list=positions]")!;
    expect(sells).toHaveTextContent(/BTC/);
    const untouched = page().querySelector("[data-list=untouched]")!;
    expect(untouched).toHaveTextContent("Agent 2, with its orders and positions");
    expect(untouched).toHaveTextContent("Your 20 ABC, which no agent manages");
  });

  it("says outside the session stock sells wait, when the kill switch would sell stock", () => {
    renderWithRuntime(<StopRecordScreen kind="stop_all" targetId={CONNECTION} />);
    const ws = buildWorkspace("normal");
    const stock = ws.agents.some((a) => a.positions.some((p) => p.instrument.asset_class === "us_equity"));
    expect(page().textContent?.includes("Outside the regular session, stock sells wait for it to open")).toBe(stock);
  });

  it("lists everything closing the account touches, including orders and holdings no agent manages (PX-12)", () => {
    renderWithRuntime(<StopRecordScreen kind="close_all" targetId={CONNECTION} />);
    expect(page().querySelector("[data-list=orders]")).toHaveTextContent("Any other open order on the account, including ones Owlhead did not place");
    expect(page().querySelector("[data-list=positions]")).toHaveTextContent("Your 20 ABC, which no agent manages");
  });

  it("shows the release warning in full: the positions become the owner's and unprotected (D11)", () => {
    renderWithRuntime(<StopRecordScreen kind="release" targetId={AGENT_IDS.btc} />);
    const warning = page().querySelector("[data-slot=release-warning]")!;
    expect(warning).toHaveTextContent("The positions become yours and unprotected");
    expect(warning).toHaveTextContent("This warning is recorded with your confirmation.");
    expect(page().querySelector("[data-list=positions]")).toHaveTextContent("After release: no protection.");
  });

  it("offers nothing to release for a flat agent", () => {
    renderWithRuntime(<StopRecordScreen kind="release" targetId={AGENT_IDS.lmn} />);
    expect(page().querySelector("[data-slot=nothing-to-do]")).toHaveTextContent("It holds no positions, so there is nothing to release.");
    expect(within(page()).queryByRole("button", { name: /Stop and release/ })).toBeNull();
  });

  it("says when there is no such agent or connection", () => {
    renderWithRuntime(<StopRecordScreen kind="kill" targetId="agt_01JB3KZZZZZZZZZZZZZZZZZZZZ" />);
    expect(page().querySelector("[data-slot=record-missing]")).toHaveTextContent("no agent with that ID");
    renderWithRuntime(<StopRecordScreen kind="close_all" targetId="con_01JB3KZZZZZZZZZZZZZZZZZZZZ" />);
    expect(document.querySelectorAll("[data-slot=record-missing]")[1]).toHaveTextContent("no broker connection with that ID");
  });

  it("shows only a skeleton while loading, so the action appears only once the full screen can render", () => {
    renderWithRuntime(<StopRecordScreen kind="kill" targetId={AGENT_IDS.btc} />, "loading");
    expect(page().querySelector("[data-slot=skeleton]")).not.toBeNull();
    expect(within(page()).queryByRole("button")).toBeNull();
    expect(within(page()).getByRole("heading", { level: 1 })).toHaveTextContent("Kill switch");
  });

  it("gives the broker route when the deployment is unreachable, and offers nothing it cannot deliver", () => {
    renderWithRuntime(<StopRecordScreen kind="kill" targetId={AGENT_IDS.btc} />, "unreachable");
    expect(page()).toHaveTextContent("Cannot reach your deployment");
    expect(page()).toHaveTextContent("sign in to your Alpaca paper dashboard");
    expect(within(page()).queryByRole("button")).toBeNull();
  });

  it("offers the action only to a role that may stop", () => {
    renderWithRuntime(<StopRecordScreen kind="kill" targetId={AGENT_IDS.btc} />, "normal", { role: "approver" });
    expect(within(page()).queryByRole("button")).toBeNull();
    expect(page()).toHaveTextContent("Stopping and closing are for an owner or operator.");
  });
});

describe("every record screen needs a passkey first (PX-4, PX-12, DEC-136)", () => {
  it.each(named)("%s opens the passkey check on the page naming the action, and sends nothing while it waits", (_, c) => {
    const { before } = start(c);
    const dialog = stepUpDialog();
    expect(dialog).not.toBeNull();
    expect(dialog!.querySelector("[data-slot=step-up-action]")).toHaveTextContent(c.stepUp);
    expectNothingSent(page(), before);
    expect(stepUpDialog()).not.toBeNull();
  });

  it.each(named)("%s asks the passkey for this one action", (_, c) => {
    const passkey = vi.fn<Passkey>(mockPasskey);
    start(c, { passkey });
    expect(passkey).not.toHaveBeenCalled();
    press("Use passkey");
    expect(passkey).toHaveBeenCalledTimes(1);
    expect(passkey).toHaveBeenCalledWith(c.stepUp, expect.any(Function));
  });
});

describe("a cancelled or failed passkey check on a record screen never becomes an action (G3, P4)", () => {
  it.each(ALL.flatMap((c) => ENDINGS.map((e) => [c.name, e.ending, c, e] as const)))("%s, ended by %s, sends nothing and says so", (_, __, c, e) => {
    const { before } = start(c, { passkey: e.passkey });
    e.run();
    expect(stepUpDialog()).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(log(page())).toHaveTextContent(e.notice);
    expectNothingSent(page(), before);
    expect(recordedCount()).toBe(0);
  });
});

describe("after a verified passkey, the record screen sends once and never shows it done early (rule 5)", () => {
  it.each(named)("%s shows sent, then recorded only once the runtime records it", (_, c) => {
    const { before } = start(c);
    press("Use passkey");
    expect(stepUpDialog()).toHaveTextContent("Waiting for your passkey…");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS - 1));
    expect(entries(page())).toHaveLength(0);
    act(() => vi.advanceTimersByTime(1));
    expect(stepUpDialog()).toBeNull();
    expectSentThenRecorded(page(), before, c.recorded);
  });

  it.each(named)("%s is sent once when Use passkey is pressed twice", (_, c) => {
    start(c);
    press("Use passkey");
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS + RECORD_AFTER_MS));
    expect(entries(page())).toHaveLength(1);
    expect(recordedCount()).toBe(1);
  });

  it.each(named)("%s sends what the owner saw with the command, so the journal keeps the record", (_, c) => {
    start(c);
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    const record = buildRecord(c.kind, buildWorkspace(c.scenario), c.target)!;
    expect(runtimeCommands()).toEqual([
      expect.objectContaining({
        kind: c.kind,
        agentId: c.kind === "kill" || c.kind === "release" ? c.target : null,
        record: {
          screen: c.kind === "release" ? "D11" : "D10",
          environment: "paper",
          title: record.title,
          shown: recordLines(record),
          modes: record.modes.map((m) => ({ agent: m.label, badge: MODE_LABEL[m.mode] })),
        },
      }),
    ]);
  });

  it.each(named)("%s stores each mode badge exactly as it read on screen when the owner confirmed", (_, c) => {
    start(c);
    const onScreen = badges(recordRegion());
    expect(onScreen.length).toBeGreaterThan(0);
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    const [command] = runtimeCommands() as Array<{ record: { modes: unknown; shown: string[] } }>;
    expect(command.record.modes).toEqual(onScreen);
    for (const { agent, badge } of onScreen) expect(command.record.shown).toContain(`${agent}: ${badge}`);
  });

  it.each(named)("%s keeps the whole record as confirmed after the runtime records it, with live progress outside it", (_, c) => {
    start(c);
    const confirmed = recordRegion().innerHTML;
    const shownModes = badges(recordRegion());
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    expect(recordRegion().innerHTML).toBe(confirmed);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    expect(entries(page())[0]).toHaveAttribute("data-phase", "recorded");
    expect(recordRegion().innerHTML).toBe(confirmed);

    const after = page().querySelector<HTMLElement>("[data-slot=after-confirm]")!;
    expect(within(after).getByRole("heading", { name: "After you confirmed" })).toBeInTheDocument();
    expect(recordRegion().contains(after)).toBe(false);
    expect(after.contains(entries(page())[0])).toBe(true);
    const live = badges(after);
    expect(live.map((m) => m.agent)).toEqual(shownModes.map((m) => m.agent));
    expect(live).not.toEqual(shownModes);
    expect(live.find((m) => m.agent === shownModes[0].agent)?.badge).toBe(MODE_LABEL.stopped);
  });

  it("opened again after the kill switch is recorded, says there is nothing more to stop", () => {
    const c = CASES.kill[1];
    const kc: KindCase = { ...c, kind: "kill" };
    const { view } = start(kc);
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS + RECORD_AFTER_MS));
    view.rerender(<Tree c={kc} shown={false} />);
    view.rerender(<Tree c={kc} />);
    expect(page().querySelector("[data-slot=nothing-to-do]")).toHaveTextContent("Stopped. There is nothing more to stop for this agent.");
    expect(within(page()).queryByRole("button", { name: c.activate })).toBeNull();
  });
});

describe("a change before the owner confirms asks for a fresh render, never a silent update (brief §4.1)", () => {
  function pauseElsewhere(c: KindCase) {
    act(() => elsewhere.send!("pause", agentOf(c)));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
  }

  it.each(named)("%s keeps the record, withdraws the action and offers the current version", (_, c) => {
    setPathname(recordHref(c.kind, c.target));
    renderWithRuntime(<Tree c={c} />, c.scenario);
    const opened = recordRegion().innerHTML;
    pauseElsewhere(c);

    expect(recordRegion().innerHTML).toBe(opened);
    const changed = page().querySelector<HTMLElement>("[data-slot=record-changed]")!;
    expect(changed).toHaveTextContent("This changed since the page opened, so the record above is out of date. Nothing was sent.");
    expect(within(page()).queryByRole("button", { name: c.activate })).toBeNull();
    expect(runtimeCommands().map((cmd) => cmd.kind)).toEqual(["pause"]);

    fireEvent.click(within(changed).getByRole("button", { name: "Show the current version" }));
    expect(page().querySelector("[data-slot=record-changed]")).toBeNull();
    const agent = buildWorkspace(c.scenario).agents.find((a) => a.agent_id === agentOf(c))!;
    expect(badges(recordRegion())).toContainEqual({ agent: agent.label, badge: MODE_LABEL.paused });

    fireEvent.click(within(page()).getByRole("button", { name: c.activate }));
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    const sent = runtimeCommands().find((cmd) => cmd.kind === c.kind) as { record: { modes: unknown } };
    expect(sent.record.modes).toEqual(badges(recordRegion()));
    expect(sent.record.modes).toContainEqual({ agent: agent.label, badge: MODE_LABEL.paused });
  });

  it.each(named)("%s closes the passkey check when the record changes while it is asked, and a late answer sends nothing", (_, c) => {
    const late: Passkey = (_action, answer) => {
      window.setTimeout(() => answer("verified"), RECORD_AFTER_MS * 2);
      return () => {};
    };
    setPathname(recordHref(c.kind, c.target));
    renderWithRuntime(<Tree c={c} />, c.scenario, { passkey: late });
    act(() => elsewhere.send!("pause", agentOf(c)));
    fireEvent.click(within(page()).getByRole("button", { name: c.activate }));
    press("Use passkey");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));

    expect(stepUpDialog()).toBeNull();
    expect(page().querySelector("[data-slot=record-changed]")).not.toBeNull();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 4));
    expect(runtimeCommands().map((cmd) => cmd.kind)).toEqual(["pause"]);
    expect(stepUpDialog()).toBeNull();
  });
});

describe("leaving a record screen mid-passkey abandons the request, and a late answer never becomes an action (P4)", () => {
  let console_: ReturnType<typeof watchConsole>;
  beforeEach(() => {
    console_ = watchConsole();
  });
  afterEach(() => console_.restore());

  const RESULTS: PasskeyResult[] = ["verified", "failed"];
  const CASES_BY_RESULT = ALL.flatMap((c) => RESULTS.map((r) => [c.name, r, c] as const));

  it.each(CASES_BY_RESULT)("%s: navigating away while the passkey is asked sends nothing, even when it later answers %s", (_, result, c) => {
    const { passkey, release } = heldPasskey(result);
    const { view } = start(c, { passkey });
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS - 1));

    view.rerender(<Tree c={c} shown={false} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));

    expect(recordedCount()).toBe(0);
    expect(release).toHaveBeenCalledTimes(1);
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
