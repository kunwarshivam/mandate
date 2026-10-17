import { describe, expect, it } from "vitest";
import { emptyWorkspace } from "./reads";

describe("the workspace before the API answers (DEC-975)", () => {
  it("shows nothing before the first answer: no agent, request, decision or holding", () => {
    const ws = emptyWorkspace("loading", "2026-10-08T14:30:00.000000000Z");
    expect(ws).toMatchObject({ status: "loading", agents: [], approvals: [], decisions: [], timeline: {}, external_positions: [], approver_users: 0, independent_approval_required: null });
    expect(emptyWorkspace("unreachable", "2026-10-08T14:30:00.000000000Z").status).toBe("unreachable");
  });

  it("names no environment and no feed time, so a live account is never labelled paper or a client clock read as a feed's (DEC-1096)", () => {
    for (const status of ["loading", "unreachable", "signed_out", "membership_unavailable"] as const) {
      const ws = emptyWorkspace(status, "2026-10-08T14:30:00.000000000Z");
      expect(ws.environment, status).toBe("unknown");
      for (const feed of Object.values(ws.health)) expect(feed.as_of, status).toBeNull();
    }
    expect(emptyWorkspace("unreachable", "2026-10-08T14:30:00.000000000Z").health.deployment.state).toBe("down");
  });
});
