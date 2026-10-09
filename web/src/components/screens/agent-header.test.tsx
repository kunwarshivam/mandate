import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import type { AgentMode, Scenario, Workspace } from "@/fixtures/types";
import { AGENT_IDS } from "@/fixtures/workspace";
import { MODE_LABEL } from "@/lib/labels";
import { AGENT_SECTIONS, type AgentSectionKey, agentHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { shownOnDesktop } from "@/test/viewport";
import { AgentDetailScreen, AgentSectionScreen } from "./agent-detail";

/**
 * C-8: an agent's mode is the first thing its page says (DEC-512), so on desktop the chip sits beside
 * the name on every tab, as it does on a phone, and the page shows it once.
 */

const TABS = AGENT_SECTIONS.filter((s) => !s.parent).map((s) => s.key);

/** Each mode the fixtures carry, and Stopped, which the kill switch reaches, set on the agent directly. */
const MODES: Array<{ mode: AgentMode; scenario: Scenario; agentId: string; set?: (ws: Workspace) => Workspace }> = [
  { mode: "normal", scenario: "normal", agentId: AGENT_IDS.swing },
  { mode: "exits_only", scenario: "drawdown", agentId: AGENT_IDS.btc },
  { mode: "paused", scenario: "paused", agentId: AGENT_IDS.swing },
  {
    mode: "stopped",
    scenario: "normal",
    agentId: AGENT_IDS.lmn,
    set: (ws) => ({ ...ws, agents: ws.agents.map((a) => (a.agent_id === AGENT_IDS.lmn ? { ...a, mode: "stopped" as const } : a)) }),
  },
];

function openTab(agentId: string, key: AgentSectionKey, scenario: Scenario, set?: (ws: Workspace) => Workspace) {
  setPathname(agentHref(agentId, key));
  const ui = key === "overview" ? <AgentDetailScreen agentId={agentId} /> : <AgentSectionScreen agentId={agentId} section={key} />;
  return renderWithRuntime(<AppShell>{ui}</AppShell>, scenario, set ? { workspace: set } : {});
}

beforeEach(() => setPathname("/"));

describe("the desktop agent header", () => {
  it.each(MODES.flatMap((m) => TABS.map((tab) => [MODE_LABEL[m.mode], tab, m] as const)))("says %s beside the name on %s", (label, tab, { scenario, agentId, set }) => {
    openTab(agentId, tab, scenario, set);
    const main = screen.getByRole("main");
    const title = within(main).getByRole("heading", { level: 1 });
    const chip = within(title.parentElement!).getByText(label, { exact: true });
    expect(shownOnDesktop(chip)).toBe(true);
  });

  it.each(TABS)("shows the mode once on desktop on %s, so Overview does not repeat it in the mandate card", (tab) => {
    openTab(AGENT_IDS.swing, tab, "paused");
    const chips = [...screen.getByRole("main").querySelectorAll("[data-slot=mode-badge]")].filter(shownOnDesktop);
    expect(chips.map((c) => c.textContent)).toEqual([MODE_LABEL.paused]);
  });
});
