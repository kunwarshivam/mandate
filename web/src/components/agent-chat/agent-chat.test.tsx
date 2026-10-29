import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { FIXTURE_ANSWER } from "@/lib/fixture-chat";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AgentScreen } from "./agent-screen";
import { ChatSettingsSection } from "./chat-settings";
import { HomeScreen } from "./home";
import { SetupScreen } from "./setup-screen";

beforeEach(() => setPathname("/"));

const composer = () => screen.getByRole("textbox");

async function say(text: string) {
  fireEvent.change(composer(), { target: { value: text } });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
}

describe("Home (DEC-2293)", () => {
  it("lists every agent as a conversation, with its last message, and marks the one an approval waits on", async () => {
    const ws = buildWorkspace("approvals");
    renderWithRuntime(<HomeScreen />, "approvals");
    await waitFor(() => expect(document.querySelectorAll("[data-slot=home-agent]").length).toBeGreaterThan(0));
    const agents = [...document.querySelectorAll("[data-slot=home-agent]")];
    expect(agents.map((a) => a.getAttribute("href"))).toEqual(ws.agents.map((a) => `/agents/${a.agent_id}`));
    const waiting = new Set(ws.approvals.filter((a) => a.status === "delivered").map((a) => a.agent_id));
    for (const row of agents) {
      const id = row.getAttribute("href")!.split("/")[2];
      expect(row.querySelector("[data-slot=needs-you]") !== null, id).toBe(waiting.has(id));
    }
    expect(screen.getAllByRole("link", { name: "New agent" })[0]).toHaveAttribute("href", "/new");
  });
});

describe("an agent's thread", () => {
  it("opens with what the agent said first, as typed cards, and its approval links to the decision", async () => {
    const ws = buildWorkspace("approvals");
    const approval = ws.approvals.find((a) => a.status === "delivered")!;
    setPathname(`/agents/${approval.agent_id}`);
    renderWithRuntime(<AgentScreen agentId={approval.agent_id} />, "approvals");
    const card = await screen.findByRole("region", { name: "Needs your OK" });
    expect(within(card).getByRole("link", { name: "Review and answer" })).toHaveAttribute("href", `/approvals/${approval.approval_id}`);
    expect(document.querySelector("[data-slot=agent-message][data-proactive]")).not.toBeNull();
  });

  it("shows the owner's message and the answer once the thread reads them back", async () => {
    setPathname(`/agents/${AGENT_IDS.swing}`);
    renderWithRuntime(<AgentScreen agentId={AGENT_IDS.swing} />);
    await screen.findByRole("heading", { level: 1 });
    await say("how am I doing?");
    await waitFor(() => expect(screen.getByText(FIXTURE_ANSWER)).toBeInTheDocument());
    const owner = document.querySelectorAll("[data-slot=owner-message]");
    expect(owner[owner.length - 1]).toHaveTextContent("how am I doing?");
  });

  it("keeps Stop's controls in the header, apart from the chat", async () => {
    setPathname(`/agents/${AGENT_IDS.swing}`);
    renderWithRuntime(<AgentScreen agentId={AGENT_IDS.swing} />);
    const header = await waitFor(() => document.querySelector("[data-slot=agent-header]")!);
    expect(within(header as HTMLElement).getByRole("button", { name: /stop$/ })).toBeInTheDocument();
  });

  it("says a stopped agent takes questions only", async () => {
    const ws = buildWorkspace("normal");
    const stopped = ws.agents.find((a) => a.mode === "stopped");
    if (!stopped) return;
    setPathname(`/agents/${stopped.agent_id}`);
    renderWithRuntime(<AgentScreen agentId={stopped.agent_id} />);
    expect(await screen.findByText("Stopped: questions only")).toBeInTheDocument();
  });

  it("opens Details with its positions, goals, mandate and activity, read only", async () => {
    setPathname(`/agents/${AGENT_IDS.swing}`);
    renderWithRuntime(<AgentScreen agentId={AGENT_IDS.swing} />);
    await act(async () => fireEvent.click(await screen.findByRole("button", { name: "Details" })));
    const drawer = await screen.findByRole("dialog", { name: "Details" });
    for (const name of ["Positions", "Goals", "Mandate", "Activity"]) expect(within(drawer).getByRole("region", { name })).toBeInTheDocument();
    await waitFor(() => expect(within(drawer).getByText("Capital")).toBeInTheDocument());
    expect(within(drawer).queryByRole("textbox")).toBeNull();
  });
});

describe("a new agent", () => {
  it("starts a setup thread on its first message", async () => {
    setPathname("/new");
    renderWithRuntime(<SetupScreen />);
    await screen.findByText(/Tell me what you want this agent to do/);
    await say("trade SPY with $1,000, carefully");
    await waitFor(() => expect(screen.getByText(FIXTURE_ANSWER)).toBeInTheDocument());
  });
});

describe("each agent's chat settings", () => {
  it("offers the deployment's models, and saves a positive cap only", async () => {
    renderWithRuntime(<ChatSettingsSection />);
    const [first] = await screen.findAllByRole("group");
    const cap = await within(first).findByLabelText("Daily AI spending cap, USD");
    await waitFor(() => expect(cap).toHaveValue("5"));
    expect(within(first).getAllByRole("option").map((o) => o.getAttribute("value"))).toEqual(["llm.chat_sonnet", "llm.chat_haiku"]);
    fireEvent.change(cap, { target: { value: "0" } });
    await act(async () => fireEvent.click(within(first).getByRole("button", { name: "Save" })));
    expect(within(first).getByRole("status")).toHaveTextContent("positive amount");
    fireEvent.change(cap, { target: { value: "2.50" } });
    await act(async () => fireEvent.click(within(first).getByRole("button", { name: "Save" })));
    await waitFor(() => expect(within(first).getByRole("status")).toHaveTextContent("Saved."));
  });
});
