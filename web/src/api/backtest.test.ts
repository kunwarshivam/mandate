import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { type BacktestRead, backtestNotFound, backtestRead, readBacktestFrom, servedBacktest } from "./backtest";
import { type Fetch, createWorkspaceClient } from "./client";
import { createMockServer } from "./mock-server";
import { BACKTESTS_BY_AGENT, BACKTEST_FIXTURES, BACKTEST_IDS } from "@/fixtures/backtests";
import { buildWorkspace } from "@/fixtures/workspace";

/**
 * The decoder is checked against the schema's own example and its variants
 * (`schemas/workspace-api/examples/read-models.backtest{,.valid,.invalid}.json`), which the schema
 * checks hold to the JSON Schema: an independent oracle, written by another lane against the schema,
 * not against this decoder (DEC-2400 item 1).
 */
const EXAMPLES = join(__dirname, "..", "..", "..", "schemas", "workspace-api", "examples");
const read = (name: string): unknown => JSON.parse(readFileSync(join(EXAMPLES, name), "utf8"));
const EXAMPLE = read("read-models.backtest.json");

interface Variant {
  label: string;
  set?: Record<string, unknown>;
  remove?: string[];
}
const VALID = read("read-models.backtest.valid.json") as Variant[];
const INVALID = read("read-models.backtest.invalid.json") as Variant[];

/**
 * The invalid variants the client reads by design: each only adds a member the schema does not name,
 * and spec §3.2 has a client ignore unknown members, so an older client keeps reading a newer
 * server. Nothing it adds is read or shown: the report renders only the members the decoder names
 * (a `hit_rate` included, DEC-1001 item 3). Listed by label, and checked to be exactly this list, so
 * a new invalid variant is refused unless it is added here on purpose.
 */
const UNKNOWN_MEMBER_ONLY = [
  "an unknown top-level member",
  "an unknown backtest member",
  "an unknown report member",
  "an unknown input digest",
  "an unknown metric",
  "an unknown decisions member",
  "an unknown outcome count",
  "an unknown row member",
  "an unknown risk member",
  "an unknown equity member",
  "an unknown equity point member",
];

/** RFC 6901: a pointer's reference tokens, unescaped. */
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

/** The schema's two maps from a reason code to a count: a new key in them is an entry, never an unknown member. */
const MAPS = ["/backtest/report/decisions/gate_reasons", "/backtest/report/decisions/hold_reasons"];

/** Only an added member: no removal, and every set pointer names a member the example lacks, outside the maps. */
function onlyAdds(variant: Variant): boolean {
  if ((variant.remove ?? []).length > 0) return false;
  return Object.keys(variant.set ?? {}).every((pointer) => {
    if (MAPS.includes(pointer.slice(0, pointer.lastIndexOf("/")))) return false;
    const path = tokens(pointer);
    let node: unknown = EXAMPLE;
    for (const token of path.slice(0, -1)) node = (node as Record<string, unknown>)[token];
    return !Object.hasOwn(node as object, path[path.length - 1]);
  });
}

const outcome = (body: unknown) => {
  const decoded = backtestRead(body, "");
  return decoded.ok ? "ok" : `${decoded.issue.problem} ${decoded.issue.path}`;
};

describe("read-models/backtest as the schema states it (DEC-1001, DEC-2400)", () => {
  it("reads the schema's example, every member as served", () => {
    const decoded = backtestRead(EXAMPLE, "");
    expect(decoded.ok).toBe(true);
    if (!decoded.ok || decoded.value.backtest.state !== "completed") throw new Error("the example is a completed run");
    const served = (EXAMPLE as { backtest: { report: unknown } }).backtest.report;
    expect(decoded.value.backtest.report).toEqual(served);
  });

  it.each(VALID.map((v) => [v.label, v] as const))("reads the valid variant: %s", (_, variant) => {
    expect(outcome(apply(EXAMPLE, variant))).toBe("ok");
  });

  it.each(INVALID.filter((v) => !UNKNOWN_MEMBER_ONLY.includes(v.label)).map((v) => [v.label, v] as const))("refuses the invalid variant: %s", (_, variant) => {
    expect(outcome(apply(EXAMPLE, variant))).not.toBe("ok");
  });

  it.each(INVALID.filter((v) => UNKNOWN_MEMBER_ONLY.includes(v.label)).map((v) => [v.label, v] as const))("ignores the unknown member of: %s (spec §3.2)", (_, variant) => {
    expect(outcome(apply(EXAMPLE, variant))).toBe("ok");
  });

  it("exempts exactly the invalid variants that only add an unknown member, and each one it names exists", () => {
    const labels = INVALID.map((v) => v.label);
    for (const label of UNKNOWN_MEMBER_ONLY) expect(labels, label).toContain(label);
    expect(new Set(labels).size, "labels are unique").toBe(labels.length);
    expect(INVALID.filter(onlyAdds).map((v) => v.label).sort()).toEqual([...UNKNOWN_MEMBER_ONLY].sort());
  });

  it("names what it refused: a JSON number for a decimal, a count past 2^53 − 1, a state's missing report", () => {
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/excess_total_return": -0.00795 } }))).toBe("decimal_number /backtest/report/excess_total_return");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/strategy/net_pnl": "120.50" } }))).toBe("not_canonical /backtest/report/strategy/net_pnl");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/decisions/count": 9007199254740992 } }))).toBe("wrong_type /backtest/report/decisions/count");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/decisions/rows/0/outputs_used": 1.5 } }))).toBe("wrong_type /backtest/report/decisions/rows/0/outputs_used");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report": null } }))).toBe("wrong_type /backtest/report");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/instrument": "8CCAE427-5DD0-45B3-B5FE-7C0C3A8EE5C4" } }))).toBe("not_canonical /backtest/report/instrument");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/research_agent": "true" } }))).toBe("wrong_type /backtest/report/research_agent");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/decisions/rows/0/at": "2026-09-02T13:30:00Z" } }))).toBe("not_canonical /backtest/report/decisions/rows/0/at");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/backtest_id": "bt/../x" } }))).toBe("not_canonical /backtest/backtest_id");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/strategy/periods_per_year": 252.5 } }))).toBe("wrong_type /backtest/report/strategy/periods_per_year");
    expect(outcome(apply(EXAMPLE, { label: "", set: { "/backtest/report/decisions/hold_reasons": [] } }))).toBe("wrong_type /backtest/report/decisions/hold_reasons");
  });

  it("decodes every fixture backtest, each answering for its own id", () => {
    for (const [id, body] of Object.entries(BACKTEST_FIXTURES)) {
      const decoded = servedBacktest(body, id);
      expect(decoded.ok ? decoded.backtest.backtest_id : decoded.error, id).toBe(id);
    }
    const states = Object.entries(BACKTEST_FIXTURES).map(([id, body]) => {
      const decoded = servedBacktest(body, id);
      return decoded.ok ? `${decoded.backtest.state}${decoded.backtest.report?.research_agent ? " research" : ""}` : "refused by the decoder";
    });
    expect(states.sort()).toEqual(["completed", "completed research", "refused", "running"]);
  });

  it("ties each listed backtest to a fixture agent, and lists no research-agent run (PX-9 (a))", () => {
    const agents = buildWorkspace("normal").agents.map((a) => a.agent_id);
    expect(Object.keys(BACKTESTS_BY_AGENT).sort()).toEqual([...agents].sort());
    const listed = Object.values(BACKTESTS_BY_AGENT).flat();
    expect(listed.sort()).toEqual([BACKTEST_IDS.lmnCompleted, BACKTEST_IDS.lmnRunning, BACKTEST_IDS.btcRefused].sort());
    for (const id of listed) {
      const decoded = servedBacktest(BACKTEST_FIXTURES[id], id);
      expect(decoded.ok && decoded.backtest.report?.research_agent === true, id).toBe(false);
    }
  });

  it("refuses a fixture body for another id, and answers an unknown id as the API's 404", () => {
    const other = servedBacktest(BACKTEST_FIXTURES[BACKTEST_IDS.lmnRunning], BACKTEST_IDS.lmnCompleted);
    expect(other.ok ? null : other.error.code).toBe("invalid_response");
    expect(other.ok ? null : other.error.decode?.path).toBe("/backtest/backtest_id");
    const missing = backtestNotFound();
    expect(missing.ok ? null : [missing.error.status, missing.error.code, missing.error.effect]).toEqual([404, "not_found", "none"]);
  });
});

const WS = "ws_01J9ZQ4M0000000000000000AB";

/** The mock API, with `rewrite` free to change a backtest answer before the client reads it. */
function api(rewrite?: (body: Record<string, unknown>) => unknown) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const routes: string[] = [];
  const fetch: Fetch = async (input, init) => {
    routes.push(new URL(input, "http://x.invalid").pathname);
    const answer = await server.fetch(input, init);
    if (!rewrite || !answer.ok) return answer;
    return new Response(JSON.stringify(rewrite((await answer.json()) as Record<string, unknown>)), { status: answer.status, headers: answer.headers });
  };
  return { client: createWorkspaceClient({ workspaceId: WS, fetch }), routes };
}

const errorOf = (read: BacktestRead) => (read.ok ? null : read.error);

describe("reading a backtest over the API (GET /backtests/{id})", () => {
  it("reads each fixture backtest from the mock API at its documented route", async () => {
    for (const id of Object.keys(BACKTEST_FIXTURES)) {
      const { client, routes } = api();
      const answer = await readBacktestFrom(client, id);
      expect(answer.ok ? answer.backtest.backtest_id : answer.error, id).toBe(id);
      expect(routes).toEqual([`/v1/workspaces/${WS}/backtests/${id}`]);
    }
  });

  it("reads a 404 as not found, never a crash", async () => {
    const answer = await readBacktestFrom(api().client, "bt_01JBZZZZZZZZZZZZZZZZZZZZZZ");
    expect(errorOf(answer)?.code).toBe("not_found");
    expect(errorOf(answer)?.status).toBe(404);
  });

  it("refuses an answer for another id than the one asked", async () => {
    const { client } = api((body) => ({ ...body, backtest: { ...(body.backtest as object), backtest_id: BACKTEST_IDS.lmnRunning } }));
    const answer = await readBacktestFrom(client, BACKTEST_IDS.lmnCompleted);
    expect(errorOf(answer)?.code).toBe("invalid_response");
    expect(errorOf(answer)?.decode?.path).toBe("/backtest/backtest_id");
  });

  it("refuses an answer it cannot read, naming the member", async () => {
    const { client } = api((body) => {
      const copy = structuredClone(body) as { backtest: { report: { strategy: { net_pnl: unknown } } } };
      copy.backtest.report.strategy.net_pnl = 42.18;
      return copy;
    });
    const answer = await readBacktestFrom(client, BACKTEST_IDS.lmnCompleted);
    expect(errorOf(answer)?.code).toBe("invalid_response");
    expect(errorOf(answer)?.decode).toMatchObject({ path: "/backtest/report/strategy/net_pnl", problem: "decimal_number" });
  });
});
