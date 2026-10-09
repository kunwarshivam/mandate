import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentSectionScreen } from "@/components/screens/agent-detail";
import { AppShell } from "@/components/shell/app-shell";
import { PASSKEY_ANSWER_MS } from "@/components/stop/step-up-dialog";
import type { Scenario, Workspace } from "@/fixtures/types";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace, findAgent } from "@/fixtures/workspace";
import { propose } from "@/lib/mandate-change";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { RuntimeProbe, probed } from "@/test/runtime-probe";
import { failingPasskey, press, stepUpDialog } from "@/test/step-up";
import type { Role } from "@/lib/roles";
import { ChangeReview } from "./change-review";

const swing = () => findAgent(probed().ws, AGENT_IDS.swing)!;

function renderEdit(scenario: Scenario = "normal", options: { role?: Role; passkey?: typeof failingPasskey; workspace?: (ws: Workspace) => Workspace } = {}) {
  setPathname(`/agents/${AGENT_IDS.swing}/mandate/edit`);
  return renderWithRuntime(
    <AppShell>
      <AgentSectionScreen agentId={AGENT_IDS.swing} section="mandate/edit" />
      <RuntimeProbe />
    </AppShell>,
    scenario,
    options,
  );
}

const form = () => screen.getByRole("form", { name: "Edit Agent 2's mandate" });
const field = (label: string) => within(form()).getByLabelText(label) as HTMLInputElement;
const review = () => document.querySelector<HTMLElement>("[data-slot=change-review]");
const phase = () => review()?.querySelector("[data-phase]")?.getAttribute("data-phase") ?? null;

function type(label: string, value: string) {
  fireEvent.change(field(label), { target: { value } });
  fireEvent.blur(field(label));
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Mandate › Edit (A6)", () => {
  it("lists the limits by group, filled in from the version in effect, with no review until a value changes", () => {
    renderEdit();
    expect(Array.from(form().querySelectorAll("legend"), (l) => l.textContent)).toEqual(["Capital and loss limits", "Size and pace", "Protection", "Approvals", "Quiet hours"]);
    expect(field("Capital").value).toBe("10000");
    expect(field("Lifetime loss limit").value).toBe("10");
    expect(field("Largest order").value).toBe("1000");
    expect(field("Approval window").value).toBe("10");
    expect(field("Two approvers above").value).toBe("");
    expect(field("Quiet hours start").value).toBe("23:00");
    expect(within(form()).queryByLabelText(/Instruments|Signal models|Environment/)).toBeNull();
    expect(review()).toBeNull();
    expect(screen.getByText("Change a value to see what it does.")).toBeInTheDocument();
  });

  it("applies a reducing version on confirm with no passkey, cancels the waiting approval, and starts the fields again from it", () => {
    renderEdit();
    type("Largest order", "800");
    expect(review()).toHaveAttribute("data-classification", "risk_reducing");
    expect(review()).toHaveTextContent("Version 3, diffed against version 2.");
    const rows = review()!.querySelectorAll("[data-slot=version-change]");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("Largest order");
    expect(rows[0]).toHaveTextContent("$1,000.00");
    expect(rows[0]).toHaveTextContent("$800.00");
    const applies = review()!.querySelector("[data-slot=change-applies]")!;
    expect(applies).toHaveTextContent("This applies when you confirm it. No passkey needed.");
    expect(applies).toHaveTextContent("1 request waiting for you is canceled when it applies");

    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm change" }));
    expect(stepUpDialog()).toBeNull();
    expect(phase()).toBe("sent");
    expect(swing().mandate.risk.max_order_usd).toBe("1000");

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase()).toBe("applied");
    expect(review()).toHaveTextContent(/Version 3 applied at \d{2}:\d{2}:\d{2}/);
    expect(within(review()!).getByRole("link", { name: "See it in Versions" })).toHaveAttribute("href", `/agents/${AGENT_IDS.swing}/mandate/versions`);
    expect(swing().mandate.risk.max_order_usd).toBe("800");
    expect(swing().versions).toHaveLength(3);
    expect(swing().versions[2].application).toMatchObject({ result: "applied", approvals_canceled: 1 });
    expect(probed().ws.approvals.find((a) => a.approval_id === APPROVAL_IDS.swingXyz)?.status).toBe("superseded");
    expect(field("Largest order").value).toBe("800");
    expect(applies).toHaveTextContent("1 request waiting for you is canceled when it applies");

    const [request] = probed().mandateChanges;
    expect(request).toMatchObject({ phase: "applied", number: 3, origin: { kind: "form" }, record: { screen: "A6", environment: "paper" } });
    expect(request.record.shown).toContain("Largest order from $1,000.00 to $800.00 (Risk-reducing)");
    expect(request.record.shown.at(-1)).toBe(`Version 3: ${swing().mandate_version}`);
  });

  it("takes a passkey for a risk-increasing version, records it as waiting, and applies it at the next safe point", () => {
    renderEdit();
    type("Largest order", "1200");
    expect(review()).toHaveAttribute("data-classification", "risk_increasing");
    expect(review()).toHaveTextContent("This raises risk, so it takes your passkey.");

    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm with passkey" }));
    expect(stepUpDialog()).toHaveTextContent("Change Agent 2's mandate: Largest order from $1,000.00 to $1,200.00.");
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    expect(phase()).toBe("sent");

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase()).toBe("waiting");
    expect(swing().mandate.risk.max_order_usd).toBe("1000");
    expect(swing().versions.at(-1)?.application).toEqual({ result: "pending" });
    expect(probed().ws.approvals.find((a) => a.approval_id === APPROVAL_IDS.swingXyz)?.status).toBe("delivered");

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 2));
    expect(phase()).toBe("applied");
    expect(swing().mandate.risk.max_order_usd).toBe("1200");
    expect(swing().versions.at(-1)?.application).toMatchObject({ result: "applied" });
  });

  it("keeps a risk-increasing version waiting while an order is in an unknown state", () => {
    renderEdit("unknown-order");
    type("Largest order", "1200");
    expect(review()).toHaveTextContent("An order is in an unknown state now, so it waits until the broker answers for that order.");
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm with passkey" }));
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS + RECORD_AFTER_MS * 10));
    expect(phase()).toBe("waiting");
    expect(swing().mandate.risk.max_order_usd).toBe("1000");
  });

  it("sends nothing when the passkey check is canceled or fails", () => {
    renderEdit("normal", { passkey: failingPasskey });
    type("Largest order", "1200");
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm with passkey" }));
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS));
    expect(review()).toHaveTextContent("Passkey check failed. Nothing was confirmed or sent.");

    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm with passkey" }));
    press("Cancel");
    expect(review()).toHaveTextContent("Passkey check canceled. Nothing was confirmed or sent.");
    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));
    expect(probed().mandateChanges).toEqual([]);
    expect(swing().versions).toHaveLength(2);
  });

  it("marks a value it cannot read, with the reason, and shows no review until it reads", () => {
    renderEdit();
    type("Largest order", "eight hundred");
    expect(field("Largest order")).toHaveAttribute("aria-invalid", "true");
    const describedBy = field("Largest order").getAttribute("aria-describedby")!.split(" ");
    expect(describedBy.map((id) => document.getElementById(id)?.textContent)).toEqual(["Dollars.", "Write an amount in dollars, in figures, like 1,200."]);
    expect(review()).toBeNull();
    expect(screen.getByText("Correct the marked value to see what the change does.")).toBeInTheDocument();
  });

  it("names the rule a version breaks and offers no confirm, and Keep as is puts the version in effect back", () => {
    renderEdit();
    type("Lifetime loss limit", "25");
    const refusals = review()!.querySelector("[data-slot=change-refusals]")!;
    expect(refusals.querySelector("[data-rule]")).toHaveAttribute("data-rule", "§4.3");
    expect(refusals).toHaveTextContent("Change the value in the form to continue.");
    expect(within(review()!).queryByRole("button", { name: /Confirm/ })).toBeNull();
    fireEvent.click(within(review()!).getByRole("button", { name: "Keep as is" }));
    expect(field("Lifetime loss limit").value).toBe("10");
    expect(review()).toBeNull();
  });

  it("starts a new review when a value changes after a version was sent", () => {
    renderEdit();
    type("Largest order", "800");
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm change" }));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    type("Largest order", "700");
    expect(phase()).toBeNull();
    expect(review()).toHaveTextContent("Version 4, diffed against version 3.");
    expect(within(review()!).getByRole("button", { name: "Confirm change" })).toBeInTheDocument();
  });

  it.each(["operator", "approver", "viewer"] as const)("shows a %s the review but no way to confirm it", (role) => {
    renderEdit("normal", { role });
    type("Largest order", "800");
    expect(review()).toHaveTextContent("Only the workspace owner can change a mandate.");
    expect(within(review()!).queryByRole("button", { name: /Confirm/ })).toBeNull();
  });
});

describe("Mandate › Edit where the workspace requires independent approval (§4.3, V-047; interim)", () => {
  const policy =
    (value: boolean | null | "absent", approverUsers?: number) =>
    (ws: Workspace): Workspace => {
      const next: Workspace = { ...ws, approver_users: approverUsers ?? ws.approver_users, independent_approval_required: value === "absent" ? null : value };
      if (value === "absent") delete (next as Partial<Workspace>).independent_approval_required;
      return next;
    };
  const SECOND_PERSON = "This workspace needs a second person to approve a change that raises risk, and that approval can't be asked for here yet, so a passkey alone can't confirm it.";

  it.each([true, null, "absent"] as const)("offers no passkey confirm for a risk-increasing version when the policy is %s, says why, and records nothing", (value) => {
    renderEdit("normal", { workspace: policy(value) });
    type("Lifetime loss limit", "15");
    expect(review()).toHaveAttribute("data-classification", "risk_increasing");
    expect(within(review()!).queryByRole("button", { name: /Confirm/ })).toBeNull();
    const applies = review()!.querySelector<HTMLElement>("[data-slot=change-applies]")!;
    expect(applies).toHaveTextContent("This can't be confirmed here, for the reason below.");
    expect(applies).not.toHaveTextContent(/passkey/i);
    const refusals = review()!.querySelector<HTMLElement>("[data-slot=change-refusals]")!;
    expect(within(refusals).getByText("This can't be confirmed:")).toBeInTheDocument();
    expect(within(refusals).getByText(SECOND_PERSON)).toHaveAttribute("data-rule", "V-047");
    expect(refusals).not.toHaveTextContent("V-047");
    expect(within(review()!).getByRole("button", { name: "Keep as is" })).toBeInTheDocument();

    act(() => vi.advanceTimersByTime((PASSKEY_ANSWER_MS + RECORD_AFTER_MS) * 3));
    expect(stepUpDialog()).toBeNull();
    expect(probed().mandateChanges).toEqual([]);
    expect(swing().versions).toHaveLength(2);
    expect(swing().mandate.capital.max_loss_from_allocation).toBe("0.1");
  });

  it.each([true, null, "absent"] as const)("applies a risk-reducing version on confirm with no passkey, as before, when the policy is %s", (value) => {
    renderEdit("normal", { workspace: policy(value) });
    type("Largest order", "800");
    expect(review()!.querySelector("[data-slot=change-refusals]")).toBeNull();
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm change" }));
    expect(stepUpDialog()).toBeNull();
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase()).toBe("applied");
    expect(swing().mandate.risk.max_order_usd).toBe("800");
  });

  it.each([true, null, "absent"] as const)("applies a neutral version on confirm, as before, when the policy is %s and two people can approve", (value) => {
    renderEdit("normal", { workspace: policy(value) });
    type("Quiet hours start", "22:00");
    expect(review()).toHaveAttribute("data-classification", "neutral");
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm change" }));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase()).toBe("applied");
    expect(swing().mandate.notifications.quiet_hours?.start).toBe("22:00");
  });

  it("takes the passkey for a risk-increasing version exactly as before when the policy is off", () => {
    renderEdit("normal", { workspace: policy(false) });
    type("Lifetime loss limit", "15");
    expect(review()).toHaveTextContent("This raises risk, so it takes your passkey.");
    expect(review()!.querySelector("[data-slot=change-refusals]")).toBeNull();
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm with passkey" }));
    press("Use passkey");
    act(() => vi.advanceTimersByTime(PASSKEY_ANSWER_MS + RECORD_AFTER_MS));
    expect(phase()).toBe("waiting");
  });

  it("with fewer than two people who can approve, refuses a neutral version and still applies a risk-reducing one (DEC-444)", () => {
    renderEdit("normal", { workspace: policy(true, 1) });
    type("Quiet hours start", "22:00");
    expect(within(review()!).queryByRole("button", { name: /Confirm/ })).toBeNull();
    expect(
      within(review()!).getByText("This workspace needs a second person to approve any change that doesn't lower risk, and no second person can approve in it, so only a change that lowers risk can be confirmed."),
    ).toHaveAttribute("data-rule", "V-047");
    type("Quiet hours start", "23:00");
    type("Largest order", "800");
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm change" }));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(phase()).toBe("applied");
  });
});

describe("the change review's progress", () => {
  function renderReview(scenario: Scenario, status?: "unreachable") {
    const ws = buildWorkspace(scenario);
    const proposal = propose(ws, findAgent(ws, AGENT_IDS.swing)!, { "/risk/max_order_usd": "800" });
    setPathname(`/agents/${AGENT_IDS.swing}/mandate/edit`);
    renderWithRuntime(
      <>
        <ChangeReview proposal={proposal} origin={{ kind: "form" }} fixHint="Change the value in the form to continue." />
        <RuntimeProbe />
      </>,
      scenario,
      { workspace: status ? (w) => ({ ...w, status }) : undefined },
    );
    fireEvent.click(within(review()!).getByRole("button", { name: "Confirm change" }));
  }

  it("says nothing changed when the deployment does not answer", () => {
    renderReview("normal", "unreachable");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    expect(phase()).toBe("undelivered");
    expect(review()).toHaveTextContent("Not delivered: your deployment did not answer, so nothing changed.");
    expect(swing().versions).toHaveLength(2);
  });

  it("says the result is unknown when no journal entry comes back, and points at Versions before trying again", () => {
    renderReview("result-unknown");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
    expect(phase()).toBe("unknown");
    expect(review()).toHaveTextContent("Look in Versions before confirming it again");
    expect(swing().versions).toHaveLength(2);
  });

  it("offers no second confirm once sent", () => {
    renderReview("normal");
    expect(within(review()!).queryByRole("button", { name: /Confirm/ })).toBeNull();
    for (const b of within(review()!).queryAllByRole("button")) expect(isDisabled(b)).toBe(false);
  });
});
