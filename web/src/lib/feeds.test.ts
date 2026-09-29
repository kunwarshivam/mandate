import { describe, expect, it } from "vitest";
import { SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { allFeedsOk } from "./feeds";

describe("feeds", () => {
  it.each(SCENARIOS.map((s) => s.id))("in %s, counts every feed answering only while the workspace is ready", (scenario) => {
    const ws = buildWorkspace(scenario);
    expect(allFeedsOk(ws)).toBe(ws.status === "ready" && Object.values(ws.health).every((f) => f.state === "ok"));
  });

  it("does not count a stale feed, an unreachable deployment, or a workspace still loading", () => {
    for (const scenario of ["stale", "unreachable", "loading"] as const) expect(allFeedsOk(buildWorkspace(scenario)), scenario).toBe(false);
    expect(allFeedsOk(buildWorkspace("normal"))).toBe(true);
  });
});
