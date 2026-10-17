import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { emptyWorkspace } from "@/api/reads";
import { alertLines } from "@/lib/attention";
import { EnvironmentBadge } from "./environment-badge";
import { StatusStrip, healthItems } from "./status-strip";

const NOW = "2026-10-08T14:30:00.000000000Z";
const CLOCK = /\d{1,2}:\d{2}/;

afterEach(cleanup);

describe("what the API does not serve is shown as not reported (DEC-1096)", () => {
  it("labels an environment nothing names as not reported, never paper or live", () => {
    render(<EnvironmentBadge environment="unknown" />);
    const badge = screen.getByText("ENVIRONMENT NOT REPORTED");
    expect(badge.getAttribute("data-environment")).toBe("unknown");
    expect(screen.queryByText("PAPER")).toBeNull();
    expect(screen.queryByText("LIVE")).toBeNull();
  });

  it("says each unserved feed is not reported, with no time read off the client clock", () => {
    const ws = { ...emptyWorkspace("ready", NOW) };
    const texts = healthItems(ws, NOW).map((i) => String(i.text));
    expect(texts).toEqual(["Market data not reported", "Deployment health not reported", "Broker not reported", "Push relay not reported"]);
    for (const text of texts) expect(text).not.toMatch(CLOCK);
    expect(alertLines(ws).map((l) => l.text)).toEqual(["Market data not reported", "Broker not reported", "Deployment not reported", "Push relay not reported"]);
  });

  it("shows an unreachable deployment with no answer on record without a time", () => {
    const ws = emptyWorkspace("unreachable", NOW);
    const deployment = healthItems(ws, NOW).find((i) => i.key === "deployment");
    expect(deployment).toMatchObject({ state: "down", text: "Deployment unreachable; agent data hidden" });
    render(<StatusStrip ws={ws} now={NOW} />);
    expect(screen.getByRole("region", { name: "System status" }).textContent).not.toMatch(CLOCK);
  });
});
