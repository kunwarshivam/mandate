import { describe, expect, it } from "vitest";
import { SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { allFeedsOk, oldestFeed } from "./feeds";

describe("feeds", () => {
  it("names the feed heard from longest ago", () => {
    expect(oldestFeed(buildWorkspace("normal"))).toEqual({ key: "relay", as_of: "2026-09-28T14:05:10-04:00" });
  });

  it.each(SCENARIOS.map((s) => s.id))("in %s, counts every feed answering only while the workspace is ready", (scenario) => {
    const ws = buildWorkspace(scenario);
    expect(allFeedsOk(ws)).toBe(ws.status === "ready" && Object.values(ws.health).every((f) => f.state === "ok"));
  });

  it("does not count a stale feed, an unreachable deployment, or a workspace still loading", () => {
    for (const scenario of ["stale", "unreachable", "loading"] as const) expect(allFeedsOk(buildWorkspace(scenario)), scenario).toBe(false);
    expect(allFeedsOk(buildWorkspace("normal"))).toBe(true);
  });
});
