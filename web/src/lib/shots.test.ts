import { createElement } from "react";
import { describe, expect, it } from "vitest";
import { HIDE_PANEL_CSS, NOT_CAPTURED, SCENARIO_AGENT } from "../../scripts/shots.mjs";
import { ScenarioSwitcher } from "@/components/dev/scenario-switcher";
import type { Agent, Scenario } from "@/fixtures/types";
import { SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { renderWithRuntime } from "@/test/harness";

const agentMap: Record<string, string | null> = SCENARIO_AGENT;
const notCaptured: Record<string, string> = NOT_CAPTURED;

/** What an owner reads on the agent's screen as its state: mode, start-up, restrictions and open orders. */
function agentState(a: Agent) {
  return JSON.stringify([a.mode, a.startup, a.restrictions, a.orders]);
}

/**
 * The agents a scenario changes, found by comparing each agent with itself under `normal`. It
 * does not read the script's map, so a wrong entry there fails against it.
 */
function affected(scenario: Scenario): string[] {
  const normal = new Map(buildWorkspace("normal").agents.map((a) => [a.agent_id, agentState(a)]));
  return buildWorkspace(scenario)
    .agents.filter((a) => normal.get(a.agent_id) !== agentState(a))
    .map((a) => a.agent_id);
}

describe("npm run shots (critique C-27, C-28)", () => {
  it("captures or names every fixture scenario, once", () => {
    const captured = Object.keys(agentMap);
    const skipped = Object.keys(notCaptured);
    expect(captured.filter((s) => skipped.includes(s))).toEqual([]);
    expect([...captured, ...skipped].sort()).toEqual(SCENARIOS.map((s) => s.id).sort());
  });

  it("names the agent every scenario affects, and every such agent exists in that scenario's workspace", () => {
    for (const [scenario, agent] of Object.entries(agentMap)) {
      const ws = buildWorkspace(scenario as Scenario);
      const changed = affected(scenario as Scenario);
      if (agent === null) {
        expect(ws.agents, `${scenario} is captured on Home alone, so its workspace has no agents`).toEqual([]);
      } else {
        expect(ws.agents.map((a) => a.agent_id), `${scenario}'s agent ${agent} is in its workspace`).toContain(agent);
        expect(changed, `${scenario} changes exactly the agent it is captured on`).toEqual([agent]);
      }
    }
  });

  it("the skipped scenarios change no agent's state", () => {
    for (const scenario of Object.keys(notCaptured)) expect(affected(scenario as Scenario), scenario).toEqual([]);
  });

  it("hides the scenario panel, which shows without the capture's style", () => {
    const { container } = renderWithRuntime(createElement(ScenarioSwitcher, { scenario: "normal", colourBlind: false }));
    const panel = container.querySelector<HTMLElement>("[data-slot=scenario-switcher]");
    expect(panel).not.toBeNull();
    expect(getComputedStyle(panel!).display).not.toBe("none");
    const style = document.head.appendChild(document.createElement("style"));
    style.textContent = HIDE_PANEL_CSS;
    expect(getComputedStyle(panel!).display).toBe("none");
    style.remove();
  });
});
