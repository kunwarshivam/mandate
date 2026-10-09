// @vitest-environment node
import { describe, expect, it } from "vitest";
import type { Agent } from "@/fixtures/types";
import { VERSION } from "@/fixtures/versions";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { mandateAt, versionInForce } from "./mandate-history";

const agentIn = (scenario: Parameters<typeof buildWorkspace>[0], id: string): Agent => buildWorkspace(scenario).agents.find((a) => a.agent_id === id)!;

describe("an agent's mandate at a version", () => {
  it("is the current document at the current version, and the first version's figures at the first", () => {
    const swing = agentIn("normal", AGENT_IDS.swing);
    expect(mandateAt(swing, VERSION.swing)).toBe(swing.mandate);
    const first = mandateAt(swing, VERSION.swingFirst);
    expect(first?.risk.max_gross_exposure_usd).toBe("1500");
    expect(first?.notifications.quiet_hours?.start).toBe("22:30");
    expect(swing.mandate.risk.max_gross_exposure_usd).toBe("2000");
  });

  it("is unknown for a version that was rejected, another agent's, or absent", () => {
    const btc = agentIn("drawdown", AGENT_IDS.btc);
    expect(mandateAt(btc, VERSION.btcRaise)).toBeNull();
    expect(mandateAt(btc, VERSION.swingFirst)).toBeNull();
    expect(mandateAt(btc, undefined)).toBeNull();
  });

  it("is unknown when the recorded changes do not lead back from the current document", () => {
    const swing = agentIn("normal", AGENT_IDS.swing);
    swing.mandate = { ...swing.mandate, risk: { ...swing.mandate.risk, max_gross_exposure_usd: "2500" } };
    expect(mandateAt(swing, VERSION.swingFirst)).toBeNull();
  });
});

describe("the version in force", () => {
  it("is the last applied at or before the time, and none before the first", () => {
    const swing = agentIn("normal", AGENT_IDS.swing);
    expect(versionInForce(swing.versions, "2026-09-22T09:29:59-04:00")).toBeNull();
    expect(versionInForce(swing.versions, "2026-09-22T09:30:00-04:00")).toBe(VERSION.swingFirst);
    expect(versionInForce(swing.versions, "2026-09-25T08:15:03-04:00")).toBe(VERSION.swingFirst);
    expect(versionInForce(swing.versions, "2026-09-25T08:15:04-04:00")).toBe(VERSION.swing);
  });

  it("never names a rejected version", () => {
    const btc = agentIn("drawdown", AGENT_IDS.btc);
    expect(versionInForce(btc.versions, "2026-09-28T14:05:00-04:00")).toBe(VERSION.btc);
  });
});
