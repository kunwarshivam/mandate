import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import type { Scenario } from "@/fixtures/types";
import { AGENT_IDS, APPROVAL_IDS } from "@/fixtures/workspace";
import { useHandoff } from "@/lib/handoff";
import type { Role } from "@/lib/roles";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { ConversationsProvider } from "./conversations";
import { MessagesScreen, type ThreadView } from "./messages-screen";

const swingThread = `/messages/${AGENT_IDS.swing}`;

function renderThread(agentId: string | null, { scenario = "approvals", view = "chat", role = "owner" }: { scenario?: Scenario; view?: ThreadView; role?: Role } = {}) {
  const path = agentId ? `/messages/${agentId}${view === "desk" ? "/desk" : ""}` : "/messages";
  setPathname(path);
  return renderWithRuntime(
    <AppShell>
      <ConversationsProvider>
        <MessagesScreen agentId={agentId} view={view} />
      </ConversationsProvider>
    </AppShell>,
    scenario,
    { role },
  );
}

const main = () => screen.getByRole("main");
const thread = () => within(main()).getByRole("log");
const field = () => within(main()).getByRole("textbox", { name: /^Ask about Agent/ });

function say(text: string) {
  fireEvent.change(field(), { target: { value: text } });
  fireEvent.keyDown(field(), { key: "Enter" });
}

beforeEach(() => setPathname("/messages"));
afterEach(() => vi.useRealTimers());

describe("the frame", () => {
  it("fills the window edge to edge, with no page gutter, content width or page footer", () => {
    renderThread(AGENT_IDS.swing);
    expect(main().className).not.toMatch(/px-\(--page-x\)|max-w-\(--content-max\)|mx-auto/);
    expect(main().querySelector("[data-slot=messages]")).not.toBeNull();
    expect(document.querySelector("[data-slot=page-footer]")).toBeNull();
    expect(main().querySelector("[data-slot=threads-pane] [data-slot=fixture-tag]")).toHaveTextContent("Fixture data");
  });

  it("keeps a heading for the page while a thread hides the list on a phone", () => {
    renderThread(AGENT_IDS.swing);
    expect(within(main()).getAllByRole("heading", { level: 1, name: "Messages" }).length).toBeGreaterThan(0);
  });

  it("gives an open thread the phone's header: the app header gives way only while a thread pane is on the page (DEC-482)", () => {
    renderThread(AGENT_IDS.swing);
    const header = document.querySelector<HTMLElement>("[data-slot=app-header]")!;
    expect(header).toHaveClass("max-lg:group-has-[[data-slot=thread-pane]]/frame:hidden");
    expect(header.closest(".group\\/frame")).not.toBeNull();
    expect(main().querySelector("[data-slot=thread-pane]")).not.toBeNull();
  });
});

describe("the thread's bar on a phone (DEC-482)", () => {
  const bar = () => main().querySelector<HTMLElement>("[data-slot=thread-bar]")!;
  const phoneOnly = (el: Element) => el.closest(".lg\\:hidden") !== null || el.classList.contains("lg:hidden");

  it("holds the way back, the agent, the other view and the paper badge, in one row", () => {
    renderThread(AGENT_IDS.swing);
    expect(bar()).not.toHaveClass("flex-wrap");
    expect(within(bar()).getByRole("link", { name: "Back to Messages" })).toHaveAttribute("href", "/messages");
    expect(within(bar()).getByRole("heading", { level: 2, name: /^Agent 2/ })).toBeInTheDocument();
    const badge = bar().querySelector<HTMLElement>("[data-slot=environment-badge]")!;
    expect(badge).toHaveTextContent("PAPER");
    expect(badge).toHaveClass("lg:hidden");
    expect(within(bar()).queryByRole("link", { name: "Open agent" })).toBeNull();
  });

  it("opens the agent from its name, over the whole of the owl and the name", () => {
    renderThread(AGENT_IDS.swing);
    const agent = within(bar()).getByRole("link", { name: "Agent 2" });
    expect(agent).toHaveAttribute("href", `/agents/${AGENT_IDS.swing}`);
    expect(agent).toHaveClass("max-lg:after:absolute", "max-lg:after:inset-0");
  });

  it.each([
    ["chat", "Desk", `${swingThread}/desk`],
    ["desk", "Chat", swingThread],
  ] as const)("on the %s, links to the other view in the bar, in place of the tabs", (view, label, href) => {
    renderThread(AGENT_IDS.swing, { view });
    const other = within(bar())
      .getAllByRole("link", { name: label })
      .filter((l) => phoneOnly(l) && !l.closest("nav"));
    expect(other.map((l) => l.getAttribute("href"))).toEqual([href]);
    expect(within(bar()).getByRole("navigation", { name: "Thread views" })).toHaveClass("max-lg:hidden");
  });
});

describe("the threads", () => {
  it("puts agents with a request waiting under Needs you, each row opening its thread", () => {
    renderThread(null);
    const threads = within(main()).getByRole("navigation", { name: "Threads" });
    const needsYou = within(threads).getByRole("region", { name: "Needs you" });
    const rows = within(needsYou).getAllByRole("link");
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row.getAttribute("href")).toMatch(/^\/messages\/agt_/);
      expect(row).toHaveTextContent(/\d+ requests?/);
    }
    expect(within(main()).getByText("Pick a thread")).toBeInTheDocument();
  });

  it("marks the open thread as the current page", () => {
    renderThread(AGENT_IDS.swing);
    const threads = within(main()).getByRole("navigation", { name: "Threads" });
    const current = within(threads)
      .getAllByRole("link")
      .filter((l) => l.getAttribute("aria-current") === "page");
    expect(current.map((l) => l.getAttribute("href"))).toEqual([swingThread]);
  });
});

describe("an agent's thread", () => {
  it("shows a waiting request as a card that opens the request, with no approve or skip of its own", () => {
    renderThread(AGENT_IDS.swing);
    const card = thread().querySelector<HTMLElement>("[data-slot=request-card][data-status=delivered]");
    expect(card).not.toBeNull();
    const request = within(card!).getByRole("link", { name: "Open the request" });
    expect(request).toHaveAttribute("href", `/approvals/${APPROVAL_IDS.swingXyz}`);
    expect(within(card!).queryByRole("button")).toBeNull();
    expect(within(main()).queryByRole("button", { name: /^(Approve|Skip)/ })).toBeNull();
  });

  it("pins the waiting request above the thread, with the static time it is skipped at", () => {
    renderThread(AGENT_IDS.swing);
    const pinned = main().querySelector<HTMLElement>("[data-slot=pinned-request]");
    expect(pinned).toHaveAttribute("href", `/approvals/${APPROVAL_IDS.swingXyz}`);
    expect(pinned).toHaveTextContent(/^Buy 2 XYZ at \$141\.30Skipped at \d\d:\d\d:\d\d ET if you do nothingReview$/);
  });

  it("draws the pinned request in 44px on a phone, the end of its sentence and Review read out rather than drawn", () => {
    renderThread(AGENT_IDS.swing);
    const pinned = main().querySelector<HTMLElement>("[data-slot=pinned-request]")!;
    expect(pinned).toHaveClass("max-lg:min-h-11");
    const readOnly = [...pinned.querySelectorAll(".max-lg\\:sr-only")].map((el) => el.textContent);
    expect(readOnly).toEqual([" if you do nothing", "Review"]);
  });

  it("pins nothing when no request is waiting", () => {
    renderThread(AGENT_IDS.btc, { scenario: "normal" });
    expect(main().querySelector("[data-slot=pinned-request]")).toBeNull();
  });

  it("offers Pause among the first questions, and pressing the chip only shows the Pause card", () => {
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    const chips = within(main()).getByRole("group", { name: "Ask about this agent" });
    fireEvent.click(within(chips).getByRole("button", { name: "Pause Agent 2" }));
    expect(within(thread()).getByRole("region", { name: "Pause Agent 2" })).toBeInTheDocument();
    expect(thread()).not.toHaveTextContent("Paused by you");
  });

  it("offers Chat and Desk as pages, each with its own address", () => {
    renderThread(AGENT_IDS.swing);
    const views = within(main()).getByRole("navigation", { name: "Thread views" });
    const links = within(views).getAllByRole("link");
    expect(links.map((l) => [l.textContent, l.getAttribute("href")])).toEqual([
      ["Chat", swingThread],
      ["Desk", `${swingThread}/desk`],
    ]);
    expect(links[0]).toHaveAttribute("aria-current", "page");
  });

  it("answers a question from the record, with links to what it read, and writes nothing", () => {
    renderThread(AGENT_IDS.swing);
    const before = thread().querySelectorAll("[data-slot=journal-line]").length;
    say("Why did it ask?");
    const answer = thread().querySelector<HTMLElement>("[data-slot=record-answer]");
    expect(answer).toHaveTextContent(/^Owlhead: /);
    expect(answer).not.toHaveTextContent(/from the record/i);
    expect(within(answer!).getByRole("link", { name: "Gate decision" })).toHaveAttribute("href", expect.stringMatching(/\/decisions\//));
    expect(within(answer!).getByRole("link", { name: "The request" })).toHaveAttribute("href", `/approvals/${APPROVAL_IDS.swingXyz}`);
    expect(thread().querySelectorAll("[data-slot=journal-line]")).toHaveLength(before);
    expect(field()).toHaveValue("");
  });

  it("draws the owner's Markdown as they formatted it", () => {
    renderThread(AGENT_IDS.swing);
    say("Compare **these**:\n\n| Agent | Cap |\n| --- | --: |\n| Agent 2 | $1,500 |\n\n```\nwhy did it ask\n```");
    const mine = thread().querySelector<HTMLElement>("[data-slot=owner-message]")!;
    expect(within(mine).getByText("these").tagName).toBe("STRONG");
    expect(within(mine).getByRole("table")).toHaveTextContent("Agent 2$1,500");
    expect(mine.querySelector("[data-slot=markdown-code] pre")).toHaveTextContent("why did it ask");
  });

  it("gives limits as a table, one row per limit", () => {
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    say("How close is it to its limits?");
    const answer = thread().querySelector<HTMLElement>("[data-slot=record-answer]")!;
    const table = within(answer).getByRole("table", { name: "Headroom under each limit" });
    expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Limit", "Headroom", "Set at", "At the limit"]);
    expect(within(table).getByRole("cell", { name: "Daily loss limit" })).toBeInTheDocument();
  });

  it("refuses to place an order from a message", () => {
    renderThread(AGENT_IDS.swing);
    say("buy 10 XYZ now");
    const answer = thread().querySelector<HTMLElement>("[data-slot=record-answer]");
    expect(answer).toHaveTextContent("Orders aren't placed from a message");
    expect(within(answer!).queryByRole("button", { name: /buy|place|order/i })).toBeNull();
  });

  it("pauses only when the owner presses Pause, which is never disabled, and links to the record once it is recorded", () => {
    vi.useFakeTimers();
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    say("pause it");
    const card = within(thread()).getByRole("region", { name: "Pause Agent 2" });
    const pause = within(card).getByRole("button", { name: "Pause" });
    expect(isDisabled(pause)).toBe(false);
    fireEvent.click(pause);
    expect(within(card).queryByRole("button", { name: "Pause" })).toBeNull();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(within(card).getByRole("link", { name: "See it in the record" })).toHaveAttribute("href", `/agents/${AGENT_IDS.swing}/activity`);
  });

  it("sends nothing when the owner says Not now", () => {
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    say("pause");
    fireEvent.click(within(thread()).getByRole("button", { name: "Not now" }));
    expect(thread()).toHaveTextContent("Not paused. Nothing was sent.");
  });

  it("sends stopping to the Stop sheet, which asks for the passkey", () => {
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    say("stop this agent");
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(within(thread()).getByRole("button", { name: "Open Stop" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("changes a limit through the same review as the Edit form, and the applied version joins the thread (DEC-483)", () => {
    vi.useFakeTimers();
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    say("Set the largest order to $800");
    const review = thread().querySelector<HTMLElement>("[data-slot=change-review]")!;
    expect(review).toHaveAttribute("data-classification", "risk_reducing");
    expect(review.previousElementSibling).toHaveTextContent("Set the largest order to $800");
    expect(review).not.toHaveTextContent("Set the largest order to $800");
    expect(review.querySelector("[data-slot=version-change]")).toHaveTextContent(/Largest order.*\$1,000\.00.*\$800\.00/);

    fireEvent.click(within(review).getByRole("button", { name: "Confirm change" }));
    expect(review.querySelector("[data-phase]")).toHaveAttribute("data-phase", "sent");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(review.querySelector("[data-phase]")).toHaveAttribute("data-phase", "applied");
    const versionLines = Array.from(thread().querySelectorAll("[data-slot=journal-line][data-kind=version]"), (l) => l.textContent);
    expect(versionLines.some((t) => t?.includes("Mandate version 3 applied: Largest order from $1,000.00 to $800.00."))).toBe(true);
    expect(thread().querySelectorAll("[data-slot=change-review]")).toHaveLength(1);
  });

  it("takes a passkey for a change that raises risk, and sends nothing when the owner keeps it as is", () => {
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    say("raise the largest order to $1,500");
    const review = thread().querySelector<HTMLElement>("[data-slot=change-review]")!;
    expect(within(review).getByRole("button", { name: "Confirm with passkey" })).toBeInTheDocument();
    fireEvent.click(within(review).getByRole("button", { name: "Keep as is" }));
    expect(thread().querySelector("[data-slot=change-review]")).toBeNull();
    expect(thread()).toHaveTextContent("Kept as is. Nothing was sent.");
  });

  it("hands the owner's words to setup when they ask for a new agent", () => {
    let held: string | null = null;
    function Probe() {
      const handoff = useHandoff();
      return (
        <button type="button" onClick={() => (held = handoff.peek())}>
          Read handoff
        </button>
      );
    }
    setPathname(swingThread);
    renderWithRuntime(
      <AppShell>
        <ConversationsProvider>
          <MessagesScreen agentId={AGENT_IDS.swing} />
          <Probe />
        </ConversationsProvider>
      </AppShell>,
      "normal",
    );
    say("create an agent that buys index funds weekly");
    fireEvent.click(within(thread()).getByRole("button", { name: "Continue in setup" }));
    fireEvent.click(screen.getByRole("button", { name: "Read handoff" }));
    expect(held).toBe("create an agent that buys index funds weekly");
  });

  it("keeps what was asked while the owner moves between Chat and Desk", () => {
    const view = renderThread(AGENT_IDS.swing);
    say("What does it hold?");
    const page = (v: ThreadView) => (
      <AppShell>
        <ConversationsProvider>
          <MessagesScreen agentId={AGENT_IDS.swing} view={v} />
        </ConversationsProvider>
      </AppShell>
    );
    expect(thread().querySelectorAll("[data-slot=owner-message]")).toHaveLength(1);
    view.rerender(page("desk"));
    expect(main().querySelector("[data-slot=thread-desk]")).not.toBeNull();
    view.rerender(page("chat"));
    expect(thread().querySelectorAll("[data-slot=owner-message]")).toHaveLength(1);
  });
});

describe("the desk", () => {
  it("lays out the request from the models to the owner, quoting model output in its own frame", () => {
    renderThread(AGENT_IDS.swing, { view: "desk" });
    const steps = [...main().querySelectorAll<HTMLElement>("[data-slot=desk-step]")];
    expect(steps.map((s) => s.dataset.role)).toEqual(["research", "trader", "risk", "autonomy", "you"]);
    const quotes = steps[0].querySelectorAll("[data-slot=model-quote]");
    expect(quotes.length).toBeGreaterThan(0);
    for (const q of quotes) expect(q).toHaveTextContent(/^Model output/);
    expect(within(steps[4]).getByRole("link", { name: /Open the request/ })).toHaveAttribute("href", `/approvals/${APPROVAL_IDS.swingXyz}`);
  });
});

describe("beside a thread", () => {
  it("shows the agent's headroom and Pause, never disabled", () => {
    renderThread(AGENT_IDS.swing, { scenario: "normal" });
    const rail = main().querySelector<HTMLElement>("[data-slot=thread-rail]")!;
    const headroom = within(rail).getByRole("region", { name: "Headroom" });
    expect(within(rail).getByRole("heading", { name: "Agent 2" })).toBeInTheDocument();
    expect(headroom).not.toHaveTextContent(/loss today|P&L|gain/i);
    const pause = within(rail).getByRole("button", { name: "Pause" });
    expect(isDisabled(pause)).toBe(false);
  });

  it("gives a viewer no Pause, in the rail or from a message", () => {
    renderThread(AGENT_IDS.swing, { scenario: "normal", role: "viewer" });
    expect(within(main()).queryByRole("button", { name: "Pause" })).toBeNull();
    expect(within(main()).queryByRole("button", { name: /^Pause Agent/ })).toBeNull();
    say("pause");
    expect(within(thread()).getByRole("region", { name: "Pause Agent 2" })).toHaveTextContent("Your role can't pause agents.");
  });
});
