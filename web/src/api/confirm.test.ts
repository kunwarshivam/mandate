import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { type Fetch, createWorkspaceClient } from "./client";
import { type ConfirmInput, prepareConfirm } from "./commands";
import { confirmAccepted, confirmStepUpDigest } from "./confirm";
import { createMockServer } from "./mock-server";
import type { ConfirmRecord } from "@/lib/mock-runtime";

/**
 * The confirm (spec §5.1) against its schemas' own examples and variants
 * (`schemas/workspace-api/examples/commands.confirm-{request,accepted}*.json`), which the schema checks
 * hold to the JSON Schema: an oracle written against the schema, not against this client (DEC-2401).
 */
const EXAMPLES = join(__dirname, "..", "..", "..", "schemas", "workspace-api", "examples");
const SCHEMAS = join(__dirname, "..", "..", "..", "schemas", "workspace-api", "commands");
const read = (dir: string, name: string): unknown => JSON.parse(readFileSync(join(dir, name), "utf8"));
const ACCEPTED = read(EXAMPLES, "commands.confirm-accepted.json") as Record<string, unknown>;
const REQUEST = read(EXAMPLES, "commands.confirm-request.json") as Record<string, unknown>;
const REQUEST_SCHEMA = read(SCHEMAS, "confirm-request.schema.json") as {
  required: string[];
  properties: {
    confirmed_paths: { items: { pattern: string } };
    classification_shown: { enum: string[] };
  };
};

interface Variant {
  label: string;
  set?: Record<string, unknown>;
  remove?: string[];
}
const VALID = read(EXAMPLES, "commands.confirm-accepted.valid.json") as Variant[];
const INVALID = read(EXAMPLES, "commands.confirm-accepted.invalid.json") as Variant[];

/**
 * The invalid variants the client reads by design: each only adds a member the schema does not name,
 * which spec §3.2 has a client ignore. Checked to be exactly the add-only variants, so a new invalid
 * variant is refused unless it is listed here on purpose.
 */
const UNKNOWN_MEMBER_ONLY = ["auto: an extra member in the body", "auto: an extra member in /as_of/0"];

function tokens(pointer: string): string[] {
  return pointer
    .split("/")
    .slice(1)
    .map((t) => t.replace(/~1/g, "/").replace(/~0/g, "~"));
}

function apply(base: unknown, variant: Variant): unknown {
  const doc = structuredClone(base);
  const at = (pointer: string): [Record<string, unknown>, string] => {
    const path = tokens(pointer);
    let node = doc as Record<string, unknown>;
    for (const token of path.slice(0, -1)) node = node[token] as Record<string, unknown>;
    return [node, path[path.length - 1]];
  };
  for (const [pointer, value] of Object.entries(variant.set ?? {})) {
    const [parent, key] = at(pointer);
    parent[key] = structuredClone(value);
  }
  for (const pointer of variant.remove ?? []) {
    const [parent, key] = at(pointer);
    delete parent[key];
  }
  return doc;
}

function has(doc: unknown, pointer: string): boolean {
  let node = doc;
  for (const token of tokens(pointer)) {
    if (typeof node !== "object" || node === null || !Object.hasOwn(node, token)) return false;
    node = (node as Record<string, unknown>)[token];
  }
  return true;
}

const onlyAdds = (v: Variant) => !v.remove && Object.keys(v.set ?? {}).every((pointer) => !has(ACCEPTED, pointer));

const WS = "ws_01J9ZQ4M0000000000000000AB";
const VERSION = `sha256:${"1f".repeat(32)}` as const;
const RECORD: ConfirmRecord = {
  screen: "A5",
  environment: "paper",
  shown: ["Your agent", "PAPER: simulated funds", `Version 1: ${VERSION}`],
  paths: ["/capital/allocation_usd", "/environment", "/universe/pinned_instruments"],
  classification: "risk_increasing",
};
const sha = (text: string) => `sha256:${createHash("sha256").update(text).digest("hex")}`;

function client(fetch: Fetch = async () => new Response(null, { status: 500 })) {
  let n = 0;
  return createWorkspaceClient({
    workspaceId: WS,
    fetch,
    newKey: () => `confirm-key-${String((n += 1)).padStart(8, "0")}`,
  });
}

describe("confirm-accepted, decoded as its schema states it (spec §5.1)", () => {
  it("decodes the schema's example", () => {
    expect(confirmAccepted(ACCEPTED, "")).toEqual({
      ok: true,
      value: {
        command_id: ACCEPTED.command_id,
        phase: "recorded",
        classification: "risk_increasing",
        applies: "next_safe_point",
        as_of: ACCEPTED.as_of,
      },
    });
  });

  it.each(VALID.map((v) => [v.label, v] as const))("decodes the valid variant %s", (_label, variant) => {
    expect(confirmAccepted(apply(ACCEPTED, variant), "")).toMatchObject({
      ok: true,
    });
  });

  it.each(INVALID.filter((v) => !UNKNOWN_MEMBER_ONLY.includes(v.label)).map((v) => [v.label, v] as const))("refuses the invalid variant %s", (_label, variant) => {
    expect(confirmAccepted(apply(ACCEPTED, variant), "")).toMatchObject({
      ok: false,
    });
  });

  it("reads exactly the add-only invalid variants, ignoring what they add (spec §3.2)", () => {
    expect(INVALID.filter(onlyAdds).map((v) => v.label)).toEqual(UNKNOWN_MEMBER_ONLY);
    for (const variant of INVALID.filter(onlyAdds)) {
      const decoded = confirmAccepted(apply(ACCEPTED, variant), "");
      expect(decoded, variant.label).toMatchObject({ ok: true });
      expect(JSON.stringify(decoded), variant.label).not.toContain("mutant_member");
    }
  });

  it("refuses a command id that is not a journal event id, and a phase other than recorded", () => {
    expect(confirmAccepted({ ...ACCEPTED, command_id: "cmd_1" }, "")).toMatchObject({ ok: false, issue: { path: "/command_id" } });
    expect(confirmAccepted({ ...ACCEPTED, phase: "applied" }, "")).toMatchObject({ ok: false, issue: { path: "/phase" } });
    expect(confirmAccepted({ ...ACCEPTED, classification: "risky" }, "")).toMatchObject({
      ok: false,
      issue: { path: "/classification", problem: "unknown_enum_value" },
    });
  });
});

describe("the confirm's body, exactly as confirm-request.schema.json requires it", () => {
  /** What `GET .../confirmation` would serve: values the client passes through and never computes. */
  const SERVED = { screen_digest: `sha256:${"5d".repeat(32)}` as const, classification: "risk_increasing" as const };
  const input = (overrides: Partial<ConfirmInput> = {}): ConfirmInput => ({ version: VERSION, agentId: null, base: null, record: RECORD, served: SERVED, stepUp: null, ...overrides });

  it("names the version in the route and sends every required member and no other, the served digest and classification as served", () => {
    const prepared = prepareConfirm(client(), input());
    expect(prepared.operation).toBe("confirm_version");
    expect(prepared.path).toBe(`/v1/workspaces/${WS}/mandate-versions/${encodeURIComponent(VERSION)}/confirm`);
    const body = JSON.parse(prepared.body) as Record<string, unknown>;
    expect(Object.keys(body).sort()).toEqual([...REQUEST_SCHEMA.required].sort());
    expect(Object.keys(body).sort()).toEqual(Object.keys(REQUEST).sort());
    expect(body).toEqual({
      agent_id: null,
      base_version: null,
      confirmed_paths: RECORD.paths,
      warnings_acknowledged: [],
      classification_shown: "risk_increasing",
      screen_digest: SERVED.screen_digest,
      record: { artifact: sha(JSON.stringify(RECORD)), ui_build: expect.stringMatching(/^sha256:[0-9a-f]{64}$/) },
      step_up: null,
    });
    expect(body.screen_digest).not.toBe(sha(JSON.stringify(RECORD.shown)));
    const pointer = new RegExp(REQUEST_SCHEMA.properties.confirmed_paths.items.pattern, "u");
    for (const path of body.confirmed_paths as string[]) expect(path, path).toMatch(pointer);
    expect(REQUEST_SCHEMA.properties.classification_shown.enum).toContain(body.classification_shown);
  });

  it("sends the served classification, not the one the record names", () => {
    const body = JSON.parse(prepareConfirm(client(), input({ served: { ...SERVED, classification: "neutral" } })).body) as Record<string, unknown>;
    expect(body.classification_shown).toBe("neutral");
  });

  it("binds a deployed agent's confirm to its agent and base version", () => {
    const base = `sha256:${"cd".repeat(32)}` as const;
    const body = JSON.parse(prepareConfirm(client(), input({ agentId: "agt_1", base })).body) as Record<string, unknown>;
    expect(body).toMatchObject({ agent_id: "agt_1", base_version: base });
  });

  it("is prepared once per gesture, so a retry sends the same key and body (API-4)", () => {
    const api = client();
    const prepared = prepareConfirm(api, input());
    expect(Object.isFrozen(prepared)).toBe(true);
    expect(prepareConfirm(api, input()).idempotency_key).not.toBe(prepared.idempotency_key);
  });

  it("is refused by the mock API as not_found: it holds no version and serves no confirmation to judge one against", async () => {
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    const api = createWorkspaceClient({ workspaceId: WS, fetch: server.fetch, baseUrl: "https://api.owlhead.ai" });
    for (const classification of ["risk_increasing", "neutral"] as const)
      expect(await api.send(prepareConfirm(api, input({ served: { ...SERVED, classification } })))).toMatchObject({ ok: false, error: { status: 404, code: "not_found", effect: "none" } });
  });
});

describe("the step-up a confirm binds (spec §5.1, §3.6)", () => {
  it("is SHA-256 of the canonical {mandate_version, agent_id, base_version}", () => {
    expect(confirmStepUpDigest(VERSION, null, null)).toBe(sha(`{"agent_id":null,"base_version":null,"mandate_version":"${VERSION}"}`));
    const base = `sha256:${"cd".repeat(32)}` as const;
    expect(confirmStepUpDigest(VERSION, "agt_1", base)).toBe(sha(`{"agent_id":"agt_1","base_version":"${base}","mandate_version":"${VERSION}"}`));
    expect(confirmStepUpDigest(VERSION, "agt_1", base)).not.toBe(confirmStepUpDigest(VERSION, "agt_2", base));
  });
});
