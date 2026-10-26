import { render, screen } from "@testing-library/react";
import { Toasty } from "@cloudflare/kumo/components/toast";
import { describe, expect, it } from "vitest";
import { type Command, type Runtime, RuntimeContext } from "@/lib/mock-runtime";
import { JournalToasts } from "./journal-toasts";

const AT = "2026-10-26T15:00:00.000Z";

/** The toasts over a runtime holding only `commands`: what the shell would raise for them. */
function toasts(commands: Command[]) {
  const runtime = { commands, responses: {}, setUps: [] } as unknown as Runtime;
  return render(
    <Toasty>
      <RuntimeContext.Provider value={runtime}>
        <JournalToasts />
      </RuntimeContext.Provider>
    </Toasty>,
  );
}

describe("journal toasts for a command the agent's runtime still answers (DEC-2140, DEC-2102 item 4)", () => {
  const resume = (patch: Partial<Command>): Command => ({ id: "cmd-1", kind: "resume", agentId: "agt_1", phase: "recorded", sentAt: AT, recordedAt: AT, awaitsRuntime: true, ...patch });

  it("raises nothing while a resume waits for the runtime, which may still refuse it", () => {
    toasts([resume({})]);
    expect(screen.queryByText(/as you asked/)).toBeNull();
  });

  it("raises nothing for a resume the runtime refused, ended, or did not confirm", () => {
    for (const settled of ["refused", "ended", "unconfirmed"] as const) {
      const view = toasts([resume({ settled })]);
      expect(screen.queryByText(/as you asked/), settled).toBeNull();
      view.unmount();
    }
  });

  it("raises its toast once the runtime applied it", async () => {
    toasts([resume({ settled: "applied" })]);
    expect(await screen.findByText("Resume, as you asked.")).toBeInTheDocument();
  });

  it("still raises a pause's toast when it is recorded, as before", async () => {
    toasts([{ id: "cmd-2", kind: "pause", agentId: "agt_1", phase: "recorded", sentAt: AT, recordedAt: AT }]);
    expect(await screen.findByText("Pause, as you asked.")).toBeInTheDocument();
  });
});
