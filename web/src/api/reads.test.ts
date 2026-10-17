import { describe, expect, it } from "vitest";
import { emptyWorkspace } from "./reads";

describe("the workspace before the API answers (DEC-975)", () => {
  it("shows nothing before the first answer: no agent, request, decision or holding", () => {
    const ws = emptyWorkspace("loading", "2026-10-08T14:30:00.000000000Z");
    expect(ws).toMatchObject({ status: "loading", agents: [], approvals: [], decisions: [], timeline: {}, external_positions: [], approver_users: 0, independent_approval_required: null });
    expect(emptyWorkspace("unreachable", "2026-10-08T14:30:00.000000000Z").status).toBe("unreachable");
  });
});
