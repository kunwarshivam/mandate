import type { ReactNode } from "react";
import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import * as agentSection from "@/app/(app)/agents/[agentId]/[...section]/page";
import * as auditScreen from "@/app/(app)/audit/[screen]/page";
import { AppShell } from "@/components/shell/app-shell";
import type { Scenario, Workspace } from "@/fixtures/types";
import { AGENT_IDS, APPROVAL_IDS } from "@/fixtures/workspace";
import { decisionHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { ApprovalRequestScreen } from "./approval-request";
import { ApprovalsInboxScreen } from "./approvals-inbox";
import { DashboardScreen } from "./dashboard";

/**
 * Critique C-6: the owner meets their own rules by their sentence, never by the machine id. The map
 * is written out here, not read from the code, so a change to the code's wording fails this test.
 */
const SENTENCE: Record<string, string> = {
  large_orders: "ask when the order value is above $900",
  low_score: "ask when the combined model score is below 0.65",
  routine: "submit without asking when the purpose is increase or open",
};

/** Any rule id, quoted or bare, as a word of its own. */
const RAW_ID = new RegExp(`\\b(${Object.keys(SENTENCE).join("|")})\\b`);

/** The swing agent's request that its rule "low_score" sent, and the gate decision that asked. */
const ASKED_DECISION = "01JB5GQAPENECFSECZP11HYGCP";

const main = () => screen.getByRole("main");

function render(path: string, ui: ReactNode, scenario: Scenario = "normal", workspace?: (ws: Workspace) => Workspace) {
  setPathname(path);
  return renderWithRuntime(<AppShell>{ui}</AppShell>, scenario, { workspace });
}

beforeEach(() => setPathname("/"));

describe("the request shows the rule that asked by its sentence (C-6)", () => {
  it.each([APPROVAL_IDS.swingXyz, APPROVAL_IDS.btc, APPROVAL_IDS.lmn])("request %s", (id) => {
    render(`/approvals/${id}`, <ApprovalRequestScreen approvalId={id} />, "approvals");
    const why = within(main()).getByRole("heading", { name: "Why you are asked" }).parentElement!;
    expect(why).toHaveTextContent(`Your rule: ${SENTENCE.low_score}.`);
    expect(main()).not.toHaveTextContent(RAW_ID);
  });
});

describe("Home never shows a rule's id (C-6)", () => {
  it.each(["normal", "approvals"] as const)("in Needs you and the rest of Home, with requests waiting (%s)", (scenario) => {
    render("/", <DashboardScreen />, scenario);
    const needsYou = within(main()).getByRole("region", { name: /^Needs you/ });
    expect(within(needsYou).getAllByRole("link").length).toBeGreaterThan(0);
    expect(needsYou).not.toHaveTextContent(RAW_ID);
    expect(main()).not.toHaveTextContent(RAW_ID);
    const decisions = within(main()).getAllByRole("list", { name: "Decisions, newest first" });
    for (const d of decisions) expect(d).toHaveTextContent(`Submitted without asking (your rule: ${SENTENCE.routine}); resting at the broker.`);
  });

  it("in its decisions once the request that asked is answered, which then read the rule's sentence", () => {
    render("/", <DashboardScreen />, "normal", (ws) => ({
      ...ws,
      approvals: ws.approvals.map((a) =>
        a.approval_id === APPROVAL_IDS.swingXyz ? { ...a, status: "expired", resolution: { at: a.deadline, text: "Skipped at the deadline. Nothing was sent." } } : a,
      ),
    }));
    const decisions = within(main()).getAllByRole("list", { name: "Decisions, newest first" });
    for (const d of decisions) expect(d).toHaveTextContent(`Asked you for approval (your rule: ${SENTENCE.low_score}).`);
    expect(main()).not.toHaveTextContent(RAW_ID);
  });
});

describe("Approvals never shows a rule's id (C-6)", () => {
  it("says what sends a request in the rules' sentences", () => {
    render("/approvals", <ApprovalsInboxScreen />, "approvals");
    const why = within(main()).getByRole("region", { name: "What sends you a request" });
    const rows = within(why).getAllByRole("listitem");
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row).toHaveTextContent(`Your rules: ${SENTENCE.large_orders}; ${SENTENCE.low_score}. Anything no rule covers asks you too.`);
    }
    expect(main()).not.toHaveTextContent(RAW_ID);
  });
});

describe("the record and the audit keep the rule's id (C-6)", () => {
  it("the audit's decisions name the rule by its id", async () => {
    render("/audit/decisions", await auditScreen.default({ params: Promise.resolve({ screen: "decisions" }) }));
    expect(main()).toHaveTextContent("Asked you for approval (your rule “low_score”).");
    expect(main()).toHaveTextContent("Submitted without asking (your rule “routine”)");
  });

  it("the gate decision's record names the rule by its id", async () => {
    const path = decisionHref(AGENT_IDS.swing, ASKED_DECISION);
    const section = path.split("/").slice(3);
    render(path, await agentSection.default({ params: Promise.resolve({ agentId: AGENT_IDS.swing, section }) }));
    expect(main()).toHaveTextContent("Asked you for approval (your rule “low_score”).");
  });
});
