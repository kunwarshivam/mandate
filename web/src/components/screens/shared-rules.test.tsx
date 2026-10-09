import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import type { Agent, AutonomyRule, Workspace } from "@/fixtures/types";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { ApprovalsInboxScreen } from "./approvals-inbox";

/**
 * Critique C-12: on the Approvals rail, agents whose rules are the same share one paragraph that
 * names them, and agents whose rules differ in any way that changes when the owner is asked never
 * do. The sentences are written out here, not read from the code, so a change to the wording or to
 * the grouping fails this test.
 */
const LARGE = "ask when the order value is above $900";
const LOW = "ask when the combined model score is below 0.65";
const BOTH = `Your rules: ${LARGE}; ${LOW}.`;
const OTHERWISE = "Anything no rule covers asks you too.";

/** Every rule id the fixture uses, quoted or bare, as a word of its own. */
const RAW_ID = /\b(large_orders|low_score|routine)\b/;

type Autonomy = Agent["mandate"]["autonomy"];

/** Sets the autonomy of the agents named by label, leaving the others as the fixture has them. */
function withAutonomy(changes: Record<string, (a: Autonomy) => Autonomy>) {
  return (ws: Workspace): Workspace => ({
    ...ws,
    agents: ws.agents.map((agent) => {
      const change = changes[agent.label];
      return change ? { ...agent, mandate: { ...agent.mandate, autonomy: change(agent.mandate.autonomy) } } : agent;
    }),
  });
}

const waits = (timeout_s: number) => (a: Autonomy): Autonomy => ({ ...a, approval: { ...a.approval, timeout_s } });

const mapRule = (id: string, change: (r: AutonomyRule) => AutonomyRule) => (a: Autonomy): Autonomy => ({
  ...a,
  rules: a.rules.map((r) => (r.id === id ? change(r) : r)),
});

/** The rail's paragraphs, one per list item, each with the agents its links name. */
function paragraphs(workspace?: (ws: Workspace) => Workspace) {
  setPathname("/approvals");
  renderWithRuntime(
    <AppShell>
      <ApprovalsInboxScreen />
    </AppShell>,
    "approvals",
    { workspace },
  );
  const why = within(screen.getByRole("main")).getByRole("region", { name: "What sends you a request" });
  expect(why).not.toHaveTextContent(RAW_ID);
  return within(why)
    .getAllByRole("listitem")
    .map((item) => ({
      agents: within(item)
        .getAllByRole("link")
        .map((l) => l.textContent),
      hrefs: within(item)
        .getAllByRole("link")
        .map((l) => l.getAttribute("href")),
      text: item.textContent ?? "",
    }));
}

beforeEach(() => setPathname("/"));

describe("the Approvals rail gives one paragraph per set of shared rules (C-12)", () => {
  it("names every agent that shares the rules in one paragraph, and states each differing wait", () => {
    const rail = paragraphs();
    expect(rail).toHaveLength(1);
    const [shared] = rail;
    expect(shared.agents).toEqual(["Agent 1", "Agent 2", "Agent 3"]);
    expect(shared.text).toContain("Agent 1, Agent 2 and Agent 3");
    expect(shared.text).toContain(`${BOTH} ${OTHERWISE}`);
    expect(shared.text).toContain("A request waits 10 min for Agent 1 and Agent 2, and 15 min for Agent 3, then is skipped.");
    expect(shared.text.split(LARGE)).toHaveLength(2);
  });

  it("keeps a link to each named agent's mandate", () => {
    const [shared] = paragraphs();
    expect(shared.hrefs).toHaveLength(3);
    for (const href of shared.hrefs) expect(href).toMatch(/^\/agents\/[^/]+\/mandate$/);
    expect(new Set(shared.hrefs).size).toBe(3);
  });

  it("states one wait once when every agent in the paragraph waits the same", () => {
    const rail = paragraphs(withAutonomy({ "Agent 3": waits(600) }));
    expect(rail).toHaveLength(1);
    expect(rail[0].text).toContain("A request waits 10 min, then is skipped.");
    expect(rail[0].text).not.toContain(" for Agent");
  });

  it("states three different waits each against its own agent", () => {
    const rail = paragraphs(withAutonomy({ "Agent 1": waits(300), "Agent 2": waits(3600) }));
    expect(rail).toHaveLength(1);
    expect(rail[0].text).toContain("A request waits 5 min for Agent 1, 1 h for Agent 2, and 15 min for Agent 3, then is skipped.");
  });

  it("never merges agents whose rules have the same ids but a different threshold", () => {
    const rail = paragraphs(withAutonomy({ "Agent 2": mapRule("large_orders", (r) => ({ ...r, when: { ...r.when, value: "500" } })) }));
    expect(rail).toHaveLength(2);
    const [first, second] = rail;
    expect(first.agents).toEqual(["Agent 1", "Agent 3"]);
    expect(first.text).toContain(BOTH);
    expect(first.text).toContain("A request waits 10 min for Agent 1, and 15 min for Agent 3, then is skipped.");
    expect(second.agents).toEqual(["Agent 2"]);
    expect(second.text).toContain(`Your rules: ask when the order value is above $500; ${LOW}.`);
    expect(second.text).not.toContain("$900");
    expect(second.text).toContain("A request waits 10 min, then is skipped.");
  });

  it("never merges agents whose rules have the same ids but a different comparison", () => {
    const rail = paragraphs(withAutonomy({ "Agent 1": mapRule("low_score", (r) => ({ ...r, when: { ...r.when, op: "lte" } })) }));
    expect(rail.map((p) => p.agents)).toEqual([["Agent 1"], ["Agent 2", "Agent 3"]]);
    expect(rail[0].text).toContain("ask when the combined model score is at or below 0.65");
  });

  it("never merges agents whose rule for what no rule covers differs", () => {
    const rail = paragraphs(withAutonomy({ "Agent 3": (a) => ({ ...a, default: "deny" }) }));
    expect(rail.map((p) => p.agents)).toEqual([["Agent 1", "Agent 2"], ["Agent 3"]]);
    expect(rail[0].text).toContain(`${BOTH} ${OTHERWISE}`);
    expect(rail[1].text).toContain(BOTH);
    expect(rail[1].text).not.toContain("Anything no rule covers");
    expect(rail[1].text).toContain("A request waits 15 min, then is skipped.");
  });

  it("never merges agents whose rules are the same but in a different order, since the first match decides", () => {
    const rail = paragraphs(withAutonomy({ "Agent 2": (a) => ({ ...a, rules: [a.rules[1], a.rules[0], ...a.rules.slice(2)] }) }));
    expect(rail.map((p) => p.agents)).toEqual([["Agent 1", "Agent 3"], ["Agent 2"]]);
  });

  it("never merges agents whose rules differ only in one that submits without asking", () => {
    const rail = paragraphs(withAutonomy({ "Agent 1": mapRule("routine", (r) => ({ ...r, when: { ...r.when, value: ["open"] } })) }));
    expect(rail.map((p) => p.agents)).toEqual([["Agent 1"], ["Agent 2", "Agent 3"]]);
    expect(rail[0].text).toContain(BOTH);
  });

  it("gives a single agent its own paragraph", () => {
    const rail = paragraphs((ws) => ({ ...ws, agents: ws.agents.filter((a) => a.label === "Agent 3") }));
    expect(rail).toHaveLength(1);
    expect(rail[0].agents).toEqual(["Agent 3"]);
    expect(rail[0].text).toContain(`${BOTH} ${OTHERWISE} A request waits 15 min, then is skipped.`);
  });
});
