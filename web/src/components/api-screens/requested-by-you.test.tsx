import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiApproval, ApiApprovals, requestedByOwner } from "./api-screens";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";

/**
 * An approval the owner asked for (DEC-2151, DEC-2858): its content's `trigger.requested_by` is
 * `owner`, its `combined_score` is null and its evidence has no outputs. The inbox and the request
 * say "Requested by you", and show no score and no model section.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const pending = buildWorkspace("approvals").approvals.filter((a) => a.status === "delivered");
const OWNED = pending[0].approval_id.replace(/^apr_/, "");
const OTHER = pending[1]?.approval_id.replace(/^apr_/, "");

const OWNER_CONTENT = { action: { side: "buy" }, trigger: { requested_by: "owner", client_id: null }, combined_score: null, evidence: { combined_score: null, outputs: [] } };

function mount(node: React.ReactNode) {
  const server = createMockServer({ workspaceId: WS, scenario: "approvals" });
  const fetch: Fetch = async (input, init) => {
    const answer = await server.fetch(input, init);
    if (new URL(input).pathname !== `/v1/workspaces/${WS}/approvals/${OWNED}`) return answer;
    const body = (await answer.json()) as { approval: Record<string, unknown> };
    return new Response(JSON.stringify({ ...body, approval: { ...body.approval, content: OWNER_CONTENT } }), { status: 200, headers: { "Content-Type": "application/json" } });
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        {node}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

afterEach(() => cleanup());

describe("an approval the owner asked for (DEC-2858)", () => {
  it("is the owner's only when the content's trigger names the owner", () => {
    expect(requestedByOwner({ content: OWNER_CONTENT })).toBe(true);
    for (const content of [{}, { trigger: null }, { trigger: [] }, { trigger: { requested_by: "agent" } }, { trigger: { requested_by: "client" } }, { requested_by: "owner" }]) expect(requestedByOwner({ content }), JSON.stringify(content)).toBe(false);
  });

  it("says Requested by you on the request, with no score and no model section", async () => {
    mount(<ApiApproval approvalId={OWNED} />);
    const said = await screen.findByText(/^Requested by you/, {}, { timeout: 5_000 });
    expect(said.textContent).toBe("Requested by you: your agent sized this order within its mandate.");
    expect(document.body.textContent).not.toMatch(/score|model output|signal model/i);
  });

  it("marks only the owner's request in the inbox", async () => {
    mount(<ApiApprovals />);
    await waitFor(() => expect(document.querySelectorAll('[data-slot="requested-by-you"]')).toHaveLength(1), { timeout: 5_000 });
    const marked = document.querySelector('[data-slot="requested-by-you"]')?.closest("li");
    expect(marked?.querySelector("a")?.getAttribute("href")).toBe(`/approvals/${OWNED}`);
    if (OTHER) expect(document.querySelector(`a[href="/approvals/${OTHER}"]`)?.closest("li")?.querySelector('[data-slot="requested-by-you"]')).toBeNull();
    expect(document.body.textContent).not.toMatch(/score/i);
  });
});
