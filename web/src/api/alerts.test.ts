import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ALERT_KINDS, type Alert, alertsRead, subjectHref } from "./alerts";

/**
 * The alerts decoder against the published schema's own example and kind table (DEC-2141): what the
 * API may serve decodes, every kind it may serve has this screen's name and the schema's class,
 * and anything else is refused, never shown.
 */
const SCHEMAS = join(__dirname, "..", "..", "..", "schemas", "workspace-api");
const example = JSON.parse(readFileSync(join(SCHEMAS, "examples", "read-models.alerts.json"), "utf8")) as Record<string, unknown>;
const schema = JSON.parse(readFileSync(join(SCHEMAS, "read-models", "alerts.schema.json"), "utf8")) as {
  $defs: { Alert: { properties: { kind: { enum: string[] } }; allOf: Array<{ if?: { properties: { kind: { enum: string[] } } } }> } };
};

const withAlert = (change: (alert: Record<string, unknown>) => void) => {
  const body = structuredClone(example) as { alerts: Array<Record<string, unknown>> };
  change(body.alerts[0]);
  return body;
};

describe("alertsRead", () => {
  it("decodes the schema's example", () => {
    const read = alertsRead(example, "");
    expect(read.ok).toBe(true);
    if (!read.ok) return;
    expect(read.value.alerts.map((a) => a.kind)).toEqual(["kill_switch", "protection"]);
    expect(read.value.alerts[0].notice?.delivery).toEqual([{ channel: "email", status: "delivered", attempt: 1, recorded_at: "2026-10-08T14:29:31.000000000Z" }]);
    expect(read.value.truncated).toBe(false);
  });

  it("names every kind the schema serves, under the schema's class, and no other", () => {
    const served = schema.$defs.Alert.properties.kind.enum;
    const info = schema.$defs.Alert.allOf[0].if?.properties.kind.enum ?? [];
    expect(Object.keys(ALERT_KINDS).sort()).toEqual([...served].sort());
    for (const kind of served) {
      expect(ALERT_KINDS[kind as keyof typeof ALERT_KINDS].class, kind).toBe(info.includes(kind) ? "info" : "safety");
    }
  });

  it.each([
    ["a kind the read never serves", (a: Record<string, unknown>) => (a.kind = "recovery_used")],
    ["a kind no alert takes", (a: Record<string, unknown>) => (a.kind = "approval_requested")],
    ["a class that is not its kind's", (a: Record<string, unknown>) => (a.class = "info")],
    ["a control stream naming an agent", (a: Record<string, unknown>) => (a.stream = { type: "control", agent_id: "agt_x", account_ref: null })],
    ["an agent stream naming no agent", (a: Record<string, unknown>) => (a.stream = { type: "agent", agent_id: null, account_ref: null })],
    ["a delivery on a pull channel", (a: Record<string, unknown>) => ((a.notice as { delivery: Array<Record<string, unknown>> }).delivery[0].channel = "web_inbox")],
    ["a missing subject", (a: Record<string, unknown>) => delete a.subject],
  ])("refuses %s", (_, change) => {
    expect(alertsRead(withAlert(change), "").ok).toBe(false);
  });

  it("links each alert to where its subject is seen", () => {
    const base: Alert = {
      alert_id: "a",
      kind: "protection",
      class: "safety",
      raised_at: "2026-10-08T14:29:00.000000000Z",
      stream: { type: "account", agent_id: null, account_ref: "ACCT1" },
      subject: { event_id: "s", event_type: "ProtectionChanged" },
      owner_command: null,
      notice: null,
    };
    expect(subjectHref(base)).toBe("/positions");
    expect(subjectHref({ ...base, stream: { type: "agent", agent_id: "agt a", account_ref: null } })).toBe("/agents/agt%20a");
    expect(subjectHref({ ...base, kind: "kill_switch", stream: { type: "control", agent_id: null, account_ref: null } })).toBe("/audit");
  });
});
