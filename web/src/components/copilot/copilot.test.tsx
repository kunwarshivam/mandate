import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConversationsProvider } from "@/components/messages/conversations";
import { MessagesScreen } from "@/components/messages/messages-screen";
import { AppShell } from "@/components/shell/app-shell";
import { AGENT_IDS } from "@/fixtures/workspace";
import { useHandoff } from "@/lib/handoff";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";

const button = () => screen.getByRole("button", { name: "Ask Owlhead" });
const panel = () => screen.getByRole("complementary", { name: "Owlhead" });
const field = () => within(panel()).getByRole("textbox", { name: "Ask Owlhead" });

function say(text: string) {
  fireEvent.change(field(), { target: { value: text } });
  fireEvent.keyDown(field(), { key: "Enter" });
}

let held: string | null | undefined;
function HandoffProbe() {
  const handoff = useHandoff();
  return (
    <button type="button" onClick={() => (held = handoff.peek())}>
      Read handoff
    </button>
  );
}

function renderAt(path: string, ui = <HandoffProbe />, role: "owner" | "viewer" | "auditor" = "owner") {
  setPathname(path);
  return renderWithRuntime(<AppShell>{ui}</AppShell>, "normal", { role });
}

beforeEach(() => {
  held = undefined;
  setPathname("/");
});
afterEach(() => vi.useRealTimers());

describe("opening Owlhead", () => {
  it("opens from the header and closes from its own button, giving focus back", () => {
    renderAt("/");
    expect(screen.queryByRole("complementary", { name: "Owlhead" })).toBeNull();
    button().focus();
    fireEvent.click(button());
    expect(button()).toHaveAttribute("aria-expanded", "true");
    expect(field()).toHaveFocus();
    fireEvent.click(within(panel()).getByRole("button", { name: "Close Owlhead" }));
    expect(screen.queryByRole("complementary", { name: "Owlhead" })).toBeNull();
    expect(button()).toHaveFocus();
  });

  it("toggles with Cmd+J or Ctrl+J, and Escape closes it", () => {
    renderAt("/");
    fireEvent.keyDown(window, { key: "j", metaKey: true });
    expect(panel()).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "j", ctrlKey: true });
    expect(screen.queryByRole("complementary", { name: "Owlhead" })).toBeNull();
    fireEvent.keyDown(window, { key: "J", ctrlKey: true });
    fireEvent.keyDown(field(), { key: "Escape" });
    expect(screen.queryByRole("complementary", { name: "Owlhead" })).toBeNull();
  });

  it("makes room for itself: the frame carries its width only while it is open", () => {
    const { container } = renderAt("/");
    const frame = container.firstElementChild as HTMLElement;
    expect(frame.style.getPropertyValue("--copilot-w")).toBe("0px");
    fireEvent.click(button());
    expect(frame.style.getPropertyValue("--copilot-w")).toBe("26rem");
  });

  it("is not offered to a role that cannot see agents", () => {
    renderAt("/audit", null, "auditor");
    expect(screen.queryByRole("button", { name: "Ask Owlhead" })).toBeNull();
  });

  it("opens from the Owlhead row at the top of Messages, which is how a phone reaches it", () => {
    renderAt(
      "/messages",
      <ConversationsProvider>
        <MessagesScreen />
      </ConversationsProvider>,
    );
    const row = screen.getByRole("navigation", { name: "Threads" }).querySelector<HTMLElement>("[data-slot=copilot-row]")!;
    expect(row).toHaveTextContent("Owlhead");
    fireEvent.click(row);
    expect(panel()).toBeInTheDocument();
    expect(row).toHaveAttribute("aria-expanded", "true");
  });
});

describe("what Owlhead looks at", () => {
  it("names the screen, and on an agent's page answers about that agent until told otherwise", () => {
    renderAt(`/agents/${AGENT_IDS.swing}`);
    fireEvent.click(button());
    const looking = panel().querySelector("[data-slot=looking-at]")!;
    expect(looking).toHaveTextContent("Looking atAgent 2");
    say("What does it hold?");
    const answer = panel().querySelector("[data-slot=record-answer]")!;
    expect(answer.textContent).toMatch(/Agent 2:/);
    expect(answer.textContent).not.toMatch(/Agent 1:|Agent 3:/);
    fireEvent.click(within(panel()).getByRole("button", { name: "Ask about all agents, not only Agent 2" }));
    expect(panel().querySelector("[data-slot=looking-at]")).toHaveTextContent("Looking atAgent 2");
    expect(within(panel()).queryByRole("button", { name: /not only Agent 2/ })).toBeNull();
  });

  it("names a screen with no agent by its title", () => {
    renderAt("/positions");
    fireEvent.click(button());
    expect(panel().querySelector("[data-slot=looking-at]")).toHaveTextContent("Looking atPositions");
  });
});

describe("asking Owlhead", () => {
  it("starts with plain questions and answers each from the record, with links to what it read", () => {
    renderAt("/");
    fireEvent.click(button());
    const starts = within(panel()).getByRole("list", { name: "Start with" });
    fireEvent.click(within(starts).getByRole("button", { name: /What needs me\?/ }));
    const answer = panel().querySelector<HTMLElement>("[data-slot=record-answer]")!;
    expect(answer).toHaveTextContent(/^From the record/);
    expect(within(answer).getAllByRole("link").length).toBeGreaterThan(0);
  });

  it("pauses an agent only when the owner presses Pause, never disabled", () => {
    vi.useFakeTimers();
    renderAt(`/agents/${AGENT_IDS.swing}`);
    fireEvent.click(button());
    say("pause it");
    const card = within(panel()).getByRole("region", { name: "Pause Agent 2" });
    const pause = within(card).getByRole("button", { name: "Pause" });
    expect(isDisabled(pause)).toBe(false);
    fireEvent.click(pause);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(within(card).getByRole("link", { name: "See it in the record" })).toBeInTheDocument();
  });

  it("refuses an order and gives no advice", () => {
    renderAt("/");
    fireEvent.click(button());
    say("should I buy more XYZ?");
    expect(panel().querySelector("[data-slot=record-answer]")).toHaveTextContent("Orders aren't placed from a message, and I don't suggest trades.");
  });

  it("hands the owner's words to setup and closes", () => {
    renderAt("/");
    fireEvent.click(button());
    say("Create an agent that buys BTC weekly with $500");
    fireEvent.click(within(panel()).getByRole("button", { name: "Continue in setup" }));
    expect(screen.queryByRole("complementary", { name: "Owlhead" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Read handoff" }));
    expect(held).toBe("Create an agent that buys BTC weekly with $500");
  });

  it("hands nothing to setup when the owner only asks for an agent", () => {
    renderAt("/");
    fireEvent.click(button());
    fireEvent.click(within(panel()).getByRole("button", { name: /Create an agent/ }));
    fireEvent.click(within(panel()).getByRole("button", { name: "Continue in setup" }));
    fireEvent.click(screen.getByRole("button", { name: "Read handoff" }));
    expect(held).toBeNull();
  });

  it("keeps nothing once it closes", () => {
    renderAt("/");
    fireEvent.click(button());
    say("What happened today?");
    expect(panel().querySelectorAll("[data-slot=owner-message]")).toHaveLength(1);
    fireEvent.click(within(panel()).getByRole("button", { name: "Close Owlhead" }));
    fireEvent.click(button());
    expect(panel().querySelectorAll("[data-slot=owner-message]")).toHaveLength(0);
    expect(panel().querySelector("[data-slot=copilot-start]")).not.toBeNull();
  });

  it("starts a new conversation on request", () => {
    renderAt("/");
    fireEvent.click(button());
    say("What happened today?");
    fireEvent.click(within(panel()).getByRole("button", { name: "New conversation" }));
    expect(panel().querySelectorAll("[data-slot=owner-message]")).toHaveLength(0);
  });
});
