import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { ComponentType } from "react";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/shell/app-shell";
import { APPROVAL_IDS } from "@/fixtures/workspace";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import type { DirectionId } from "./directions";
import { KeelApproval, KeelDashboard } from "./keel";
import { PlacardApproval, PlacardDashboard } from "./placard";
import { VernierApproval, VernierDashboard } from "./vernier";

const VIEWS: Array<[DirectionId, ComponentType, ComponentType<{ approvalId: string }>]> = [
  ["vernier", VernierDashboard, VernierApproval],
  ["placard", PlacardDashboard, PlacardApproval],
  ["keel", KeelDashboard, KeelApproval],
];

const main = () => screen.getByRole("main");

beforeEach(() => setPathname("/directions"));

describe.each(VIEWS)("%s", (id, Dashboard, Approval) => {
  describe("dashboard", () => {
    it("keeps the shell's Stop and paper badge, and shows every agent", () => {
      renderWithRuntime(
        <AppShell>
          <Dashboard />
        </AppShell>,
      );
      const [header] = screen.getAllByRole("banner");
      expect(within(header).getByText("PAPER")).toBeInTheDocument();
      expect(isDisabled(within(header).getByRole("button", { name: "Stop" }))).toBe(false);
      for (const label of ["Agent 1", "Agent 2", "Agent 3"]) expect(within(main()).getAllByText(label).length).toBeGreaterThan(0);
    });

    it("states gains and losses in words, beside the performance placeholder", () => {
      renderWithRuntime(<Dashboard />);
      expect(screen.getAllByText("gain").length).toBeGreaterThan(0);
      expect(screen.getAllByText("loss").length).toBeGreaterThan(0);
      expect(document.querySelectorAll("[data-direction-of-change=loss]").length).toBeGreaterThan(0);
      expect(screen.getAllByText(/DISCLOSURE-PERFORMANCE/).length).toBeGreaterThan(0);
    });

    it("draws limits as rails labelled in dollars", () => {
      renderWithRuntime(<Dashboard />);
      const rails = document.querySelectorAll("[data-slot=limit-rail] [role=img]");
      expect(rails.length).toBeGreaterThanOrEqual(6);
      for (const r of rails) expect(r.getAttribute("aria-label")).toMatch(/\$[\d,]+\.\d{2} of a \$[\d,]+\.\d{2} limit/);
    });

    it("has one action when there are no agents", () => {
      renderWithRuntime(<Dashboard />, "empty");
      expect(screen.getByRole("heading", { name: "No agents yet" })).toBeInTheDocument();
      expect(screen.getByRole("link", { name: /Describe your first agent/ })).toHaveAttribute("href", "/agents/new");
    });
  });

  describe("approval request", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    const request = () => renderWithRuntime(<Approval approvalId={APPROVAL_IDS.btc} />, "approvals");

    it("gives Approve and Skip the same weight, with no default focus", () => {
      request();
      const choices = document.querySelector("[data-slot=approval-choices]") as HTMLElement;
      const [approve, skip] = within(choices).getAllByRole("button");
      expect(approve).toHaveTextContent("Approve");
      expect(skip).toHaveTextContent("Skip");
      expect(approve.className).toBe(skip.className);
      for (const b of [approve, skip]) {
        expect(b).not.toHaveAttribute("autofocus");
        expect(document.activeElement).not.toBe(b);
      }
    });

    it("states the default, the trigger, the risk in dollars, and the score's meaning", () => {
      request();
      const article = screen.getByRole("article");
      expect(article).toHaveTextContent("If you do nothing, this action is skipped.");
      expect(article).toHaveTextContent("Your rule “low_score”: ask when the combined model score is below 0.65.");
      expect(article).toHaveTextContent("$850.50");
      expect(article).toHaveTextContent("Combined model score, not a probability of profit");
      expect(article.querySelector("[data-slot=deadline]")).toHaveTextContent("Skipped at 14:13:40 ET if you do nothing");
    });

    it("keeps model output behind a control", () => {
      request();
      expect(screen.queryByText(/Output of software you selected/)).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: "View model output" }));
      expect(screen.getByText(/Output of software you selected/)).toBeInTheDocument();
    });

    it("shows nothing as approved until the runtime records it", () => {
      request();
      fireEvent.click(screen.getByRole("button", { name: "Approve" }));
      const article = screen.getByRole("article");
      expect(article).toHaveTextContent("Not recorded yet; if the runtime does not record it before the deadline (14:13:40), the action is skipped.");
      expect(article).not.toHaveTextContent(/\bapproved\b|submitted/i);
      act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
      expect(article.querySelector("[data-phase=recorded]")).toHaveTextContent(/Recorded at \d{2}:\d{2}:\d{2}: you approved/);
    });
  });

  it("never uses the kill switch's crimson", () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), `${id}.tsx`), "utf8");
    expect(source).not.toMatch(/crimson/);
  });
});
