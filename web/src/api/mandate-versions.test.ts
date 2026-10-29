import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createWorkspaceClient } from "./client";
import { REJECTION_REASONS, mandateVersionsRead, mandateVersionsRoute } from "./mandate-versions";
import { createMockServer } from "./mock-server";
import { buildWorkspace } from "@/fixtures/workspace";

/**
 * The mandate-versions read (§4.7, DEC-2147, DEC-2148) against its schema's own example and variants
 * (`schemas/workspace-api/examples/read-models.agent-mandate-versions*.json`), which the schema checks
 * hold to the JSON Schema: an oracle written against the schema, not against this client. Each body
 * goes through the client, so the envelope is read as every read's is.
 */
const ROOT = join(__dirname, "..", "..", "..", "schemas", "workspace-api");
const json = (path: string): unknown => JSON.parse(readFileSync(join(ROOT, path), "utf8"));
const BASE = json("examples/read-models.agent-mandate-versions.json") as Record<string, unknown>;
const SCHEMA = json("read-models/agent-mandate-versions.schema.json") as { $defs: { Application: { oneOf: Array<{ properties: { reason: { enum?: string[] } } }> } } };

interface Variant {
  label: string;
  set?: Record<string, unknown>;
  remove?: string[];
}
const VALID = json("examples/read-models.agent-mandate-versions.valid.json") as Variant[];
const INVALID = json("examples/read-models.agent-mandate-versions.invalid.json") as Variant[];

/** The invalid variants the client reads by design: each adds only a member the schema does not name, which spec §3.2 has a client ignore. */
const UNKNOWN_MEMBER_ONLY = ["an unknown top-level member", "an unknown member on the deployment", "an unknown member on a confirmation", "an unknown application member", "an unknown rejected application member"];

const WS = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const AGENT = "agt_demo_spy";

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

/** The read through the client, with `body` as the deployment's answer. */
async function served(body: unknown) {
  const client = createWorkspaceClient({
    workspaceId: WS,
    fetch: async () => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } }),
    newKey: () => "versions-key-00000001",
  });
  return client.read(mandateVersionsRoute(AGENT), mandateVersionsRead);
}

const versions = (doc: unknown) => (doc as { versions: Array<Record<string, unknown>> }).versions;

describe("the mandate-versions read, decoded as its schema states it (§4.7)", () => {
  it("decodes the schema's example: the deployment, then each later confirmation oldest first, and the version in effect", async () => {
    const read = await served(BASE);
    expect(read.ok).toBe(true);
    if (!read.ok) return;
    const [deployment, ...later] = versions(BASE);
    expect(read.value.agent_id).toBe(AGENT);
    expect(read.value.account_ref).toBe(BASE.account_ref);
    expect(read.value.in_effect).toBe(BASE.in_effect);
    expect(read.value.stopped_at).toBeNull();
    expect(read.value.deployed).toEqual({ kind: "deployed", mandate_version: deployment.mandate_version, classification: "first_version", created_at: deployment.created_at, confirmed_at: deployment.confirmed_at, confirmation_id: deployment.confirmation_id, deployed_at: deployment.deployed_at });
    expect(read.value.later.map((v) => [v.mandate_version, v.base_version, v.classification, v.application?.result ?? null])).toEqual(later.map((v) => [v.mandate_version, v.base_version, v.classification, (v.application as { result: string } | null)?.result ?? null]));
  });

  it.each(VALID.map((v) => [v.label, v] as const))("decodes the valid variant %s", async (_label, variant) => {
    expect(await served(apply(BASE, variant))).toMatchObject({ ok: true });
  });

  it.each(INVALID.filter((v) => !UNKNOWN_MEMBER_ONLY.includes(v.label)).map((v) => [v.label, v] as const))("refuses the invalid variant %s", async (_label, variant) => {
    expect(await served(apply(BASE, variant))).toMatchObject({ ok: false });
  });

  it("reads the unknown-member variants, each adding only `extra`, and ignores what they add (spec §3.2)", async () => {
    for (const label of UNKNOWN_MEMBER_ONLY) {
      const variant = INVALID.find((v) => v.label === label);
      expect(variant, label).toBeDefined();
      if (!variant) continue;
      expect(JSON.stringify(variant.set), label).toMatch(/extra/);
      expect(variant.remove, label).toBeUndefined();
      const read = await served(apply(BASE, variant));
      expect(read, label).toMatchObject({ ok: true });
      expect(JSON.stringify(read), label).not.toContain("extra");
    }
  });

  it("knows exactly the schema's rejection reasons", () => {
    expect([...REJECTION_REASONS].sort()).toEqual([...(SCHEMA.$defs.Application.oneOf[1].properties.reason.enum ?? [])].sort());
  });
});

describe("the history in the spec's order, refused whole when it is not (DEC-2148)", () => {
  it("refuses a history whose first entry is a confirmation, or whose later entry is a deployment", async () => {
    const [deployment, ...later] = versions(BASE);
    expect(await served({ ...BASE, versions: later })).toMatchObject({ ok: false, error: { code: "invalid_response" } });
    expect(await served({ ...BASE, versions: [deployment, deployment] })).toMatchObject({ ok: false });
  });

  it("refuses a version in effect the history does not list, and takes null", async () => {
    expect(await served({ ...BASE, in_effect: `sha256:${"9d".repeat(32)}` })).toMatchObject({ ok: false });
    expect(await served({ ...BASE, in_effect: null })).toMatchObject({ ok: true, value: { in_effect: null } });
  });
});

describe("the mock server serves the read from the fixture agents", () => {
  it("serves each fixture agent's versions as a body the client reads, and not_found for an agent it does not hold", async () => {
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    const client = createWorkspaceClient({ workspaceId: WS, fetch: server.fetch, newKey: () => "versions-key-00000002" });
    for (const agent of buildWorkspace("normal").agents) {
      const read = await client.read(mandateVersionsRoute(agent.agent_id), mandateVersionsRead);
      expect(read, agent.agent_id).toMatchObject({ ok: true, value: { agent_id: agent.agent_id, in_effect: agent.mandate_version } });
      if (read.ok) expect(1 + read.value.later.length).toBe(agent.versions.length);
    }
    expect(await client.read(mandateVersionsRoute("agt_none"), mandateVersionsRead)).toMatchObject({ ok: false, error: { code: "not_found" } });
  });
});
