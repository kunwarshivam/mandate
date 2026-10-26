import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiApproval } from "./api-screens";
import { responseSteps } from "@/components/approvals/response-progress";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

/** A pending request, named as the API names it: a bare ULID. */
const delivered = buildWorkspace("approvals").approvals.filter((a) => a.status === "delivered");
const approval = delivered
  .map((a) => ({ ...a, approval_id: a.approval_id.replace(/^apr_/, "") }))[0];

type Answer = (route: string, init: RequestInit, served: () => Promise<Response>) => Promise<Response>;

/** Mounts the approval screen over the mock deployment, with `answer` in front of it; returns every POST. */
function mount(answer: Answer) {
  const server = createMockServer({ workspaceId: WS, scenario: "approvals", hostConfirms: true });
  const posts: string[] = [];
  const fetch: Fetch = (input, init) => {
    const route = new URL(input).pathname.slice(`/v1/workspaces/${WS}`.length);
    if (init.method === "POST") posts.push(route);
    return answer(route, init, () => server.fetch(input, init));
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <ApiApproval approvalId={approval.approval_id} />
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts, answers: () => posts.filter((p) => p.endsWith("/responses")) };
}

/** The runtime's refusal of the grant, as `GET /commands/{event_id}` folds `ApprovalResponded` refused (spec §5.5). */
const refusedBy =
  (reason: string): Answer =>
  async (route, init, served) => {
    const answer = await served();
    if (init.method !== "GET" || !route.startsWith("/commands/") || !answer.ok) return answer;
    const body = (await answer.json()) as Record<string, unknown>;
    const step = { stream_id: "agent", seq: 9, event_type: "ApprovalResponded", recorded_at: "2026-10-26T15:00:00.000000000Z", reason };
    return new Response(JSON.stringify({ ...body, phase: "refused", steps: [step] }), { status: 200 });
  };

/** Approves through the host confirmation, as the owner does. */
async function approve() {
  fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
  const panel = await screen.findByRole("region", { name: "Confirm on the host" });
  fireEvent.click(within(panel).getByRole("button", { name: "Approve" }));
}

afterEach(() => {
  cleanup();
});

describe("a refused grant on the approval screen (DEC-2129 item 7, DEC-2818)", () => {
  it("a grant the runtime refused not_an_approver says so in words, never as approved, and is not sent again", async () => {
    const { answers } = mount(refusedBy("not_an_approver"));
    await approve();
    const status = await screen.findByText(/^Refused: /);
    expect(status.textContent).toMatch(/not an approver/);
    expect(status.getAttribute("data-phase")).toBe("refused");
    expect(screen.queryByText(/^Recorded in the journal: you approved/)).toBeNull();
    expect(screen.queryByText(/^Approved/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    await new Promise((settle) => setTimeout(settle, 50));
    expect(answers()).toHaveLength(1);
  });

  it("every refusal the runtime journals ends the same way: in words, with no Approve left to press", async () => {
    for (const reason of ["late", "step_up_stale", "a_reason_from_later"]) {
      const { answers } = mount(refusedBy(reason));
      await approve();
      expect((await screen.findByText(/^Refused/)).textContent, reason).toMatch(/Nothing (more )?was approved|does not count/);
      expect(screen.queryByRole("button", { name: "Approve" }), reason).toBeNull();
      expect(answers(), reason).toHaveLength(1);
      cleanup();
    }
  });

  it("a grant the API refused from someone the mandate does not list says so, and offers nothing to send again", async () => {
    const forbidden = () => Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 403, title: "Request refused", code: "forbidden", effect: "none", event_id: null, retryable: false }), { status: 403 }));
    const { answers } = mount((route, init, served) => (init.method === "POST" && route.endsWith("/responses") ? forbidden() : served()));
    await approve();
    const status = await screen.findByText(/^Not recorded: /);
    expect(status.textContent).toMatch(/not an approver/);
    expect(screen.queryByText(/^Recorded in the journal/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Skip" })).toBeNull();
    await new Promise((settle) => setTimeout(settle, 50));
    expect(answers()).toHaveLength(1);
  });

  it("a grant the runtime admitted still reads as recorded, then approved", async () => {
    mount((_route, _init, served) => served());
    await approve();
    expect(await screen.findByText(/^Approved: the gate re-ran its checks/)).toBeTruthy();
    expect(screen.queryByText(/^Refused/)).toBeNull();
  });

  it("the progress of a refused answer is recorded, and its result is the refusal, never an approval", () => {
    const record = { screen: "D6" as const, environment: "paper" as const, shown: [], modelOutputExpanded: false };
    const steps = responseSteps(delivered[0], { approvalId: delivered[0].approval_id, response: "approve", phase: "refused", reason: "not_an_approver", sentAt: delivered[0].deadline, record });
    expect(steps.map((step) => [step.key, step.label, step.state])).toEqual([
      ["sent", "Sent", "done"],
      ["recorded", "Recorded in the journal", "done"],
      ["result", "Refused", "done"],
    ]);
  });
});
