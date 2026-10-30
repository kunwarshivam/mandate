import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createWorkspaceClient } from "./client";
import { createMockServer } from "./mock-server";
import { DASHBOARD_ROUTE, HISTORY_RANGES, type HistoryRange, dashboardRead, historyRead, historyRoute, pnlRead, pnlRoute } from "./portfolio";
import type { Decoder } from "./types";
import { AGENT_IDS } from "@/fixtures/workspace";
import { INSTRUMENTS } from "@/fixtures/mandates";

/**
 * The Portfolio page's reads (§4.7, DEC-2413) against their schemas' own examples and variants
 * (`schemas/workspace-api/examples/read-models.{dashboard,equity-history,pnl}{,.valid,.invalid}.json`),
 * read through the client so the envelope is checked as every read's is: an oracle written against
 * the schemas, not this client.
 */
const ROOT = join(__dirname, "..", "..", "..", "schemas", "workspace-api");
const json = (path: string): unknown => JSON.parse(readFileSync(join(ROOT, path), "utf8"));

interface Variant {
  label: string;
  set?: Record<string, unknown>;
  remove?: string[];
}

/** RFC 6901 pointers into a copy of the example: `set` writes the member, `remove` drops it. */
function apply(base: unknown, variant: Variant): unknown {
  const doc = structuredClone(base);
  const walk = (pointer: string): [Record<string, unknown> | unknown[], string] => {
    const keys = pointer.slice(1).split("/").map((k) => k.replace(/~1/g, "/").replace(/~0/g, "~"));
    let at = doc as Record<string, unknown>;
    for (const key of keys.slice(0, -1)) at = at[key] as Record<string, unknown>;
    return [at, keys[keys.length - 1]];
  };
  for (const [pointer, value] of Object.entries(variant.set ?? {})) {
    const [parent, key] = walk(pointer);
    (parent as Record<string, unknown>)[key] = structuredClone(value);
  }
  for (const pointer of variant.remove ?? []) {
    const [parent, key] = walk(pointer);
    delete (parent as Record<string, unknown>)[key];
  }
  return doc;
}

/**
 * The invalid variants the client reads by design. An unknown member is one §3.2 has a client
 * ignore; an empty `as_of` is the envelope's own rule, which the client's envelope reader reads for
 * every read alike and none of these figures depends on.
 */
const READ_BY_DESIGN = new Set(["an unknown top-level member", "an unknown account member", "an unknown snapshot member", "an unknown point member", "an unknown instrument member", "an empty as_of"]);

const WS = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y";

async function served<T>(route: `/${string}`, decoder: Decoder<T>, body: unknown) {
  const client = createWorkspaceClient({ workspaceId: WS, fetch: async () => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } }), newKey: () => "portfolio-key-000001" });
  return client.read(route, decoder);
}

const CASES = [
  { name: "dashboard", route: DASHBOARD_ROUTE, decoder: dashboardRead },
  { name: "equity-history", route: historyRoute("1w"), decoder: null },
  { name: "pnl", route: pnlRoute("agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z"), decoder: pnlRead("agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z") },
] as const;

/** The history decoder for the range a body names, so each range's valid variant is read as the read for that range. */
const historyFor = (body: unknown): Decoder<unknown> => historyRead(((body as { range?: HistoryRange }).range ?? "1w") as HistoryRange);

describe.each(CASES)("read-models/$name, decoded as its schema states it (DEC-2413)", ({ name, route, decoder }) => {
  const base = json(`examples/read-models.${name}.json`);
  const valid = json(`examples/read-models.${name}.valid.json`) as Variant[];
  const invalid = json(`examples/read-models.${name}.invalid.json`) as Variant[];
  const decode = (body: unknown) => decoder ?? historyFor(body);

  it("decodes the example", async () => {
    expect(await served(route, decode(base), base)).toMatchObject({ ok: true });
  });

  it.each(valid.map((v) => [v.label, v] as const))("decodes the valid variant %s", async (_label, variant) => {
    const body = apply(base, variant);
    expect(await served(route, decode(body), body)).toMatchObject({ ok: true });
  });

  it.each(invalid.filter((v) => !READ_BY_DESIGN.has(v.label)).map((v) => [v.label, v] as const))("refuses the invalid variant %s", async (_label, variant) => {
    const body = apply(base, variant);
    expect(await served(route, decode(body), body)).toMatchObject({ ok: false, error: { code: "invalid_response" } });
  });

  it("reads past a member the schema does not name, and never shows it", async () => {
    for (const variant of invalid.filter((v) => READ_BY_DESIGN.has(v.label) && v.label !== "an empty as_of")) {
      const body = apply(base, variant);
      const read = await served(route, decode(body), body);
      expect(read, variant.label).toMatchObject({ ok: true });
      expect(JSON.stringify(read.ok ? read.value : null), variant.label).not.toContain("extra");
    }
  });
});

describe("the dashboard's snapshots", () => {
  const base = json("examples/read-models.dashboard.json");

  it("keeps every figure as the decimal served, and an account with no snapshot as null, never zero", async () => {
    const read = await served(DASHBOARD_ROUTE, dashboardRead, base);
    expect(read).toMatchObject({
      ok: true,
      value: {
        accounts: [
          { account_ref: "acct_1", snapshot: { equity: "10069.97", cash: "9139.97", buying_power: "9139.97", last_equity: "10000", day_change: "69.97", environment: "paper", observed_at: "2026-10-08T14:29:50.000000000Z" } },
          { account_ref: "acct_2", snapshot: null },
        ],
      },
    });
  });

  it("reads a version 1 snapshot's prior close and day change as not reported", async () => {
    const body = apply(base, { label: "v1", set: { "/accounts/0/snapshot/last_equity": null, "/accounts/0/snapshot/day_change": null } });
    expect(await served(DASHBOARD_ROUTE, dashboardRead, body)).toMatchObject({ ok: true, value: { accounts: [{ snapshot: { last_equity: null, day_change: null } }, {}] } });
  });

  it("refuses an account served twice, so none is summed twice", async () => {
    const body = apply(base, { label: "twice", set: { "/accounts/1/account_ref": "acct_1" } });
    expect(await served(DASHBOARD_ROUTE, dashboardRead, body)).toMatchObject({ ok: false, error: { decode: { path: "/accounts/1/account_ref", problem: "not_canonical" } } });
  });

  it("refuses a decimal with a trailing zero or a sign on zero, a null equity, and a snapshot of no shape", async () => {
    for (const [pointer, value] of [
      ["/accounts/0/snapshot/equity", "10069.970"],
      ["/accounts/0/snapshot/day_change", "-0"],
      ["/accounts/0/snapshot/equity", null],
      ["/accounts/0/snapshot/cash", null],
      ["/accounts/0/snapshot", []],
      ["/accounts/0", null],
    ] as const) {
      const body = apply(base, { label: pointer, set: { [pointer]: value } });
      expect(await served(DASHBOARD_ROUTE, dashboardRead, body), `${pointer} ${JSON.stringify(value)}`).toMatchObject({ ok: false });
    }
  });
});

describe("the equity history", () => {
  const base = json("examples/read-models.equity-history.json") as { range: HistoryRange };

  it("reads the points oldest first, as served", async () => {
    expect(await served(historyRoute("1w"), historyRead("1w"), base)).toMatchObject({
      ok: true,
      value: { range: "1w", accounts: [{ account_ref: "acct_1", points: [{ at: "2026-10-07T20:59:50.000000000Z", equity: "10000" }, { at: "2026-10-08T14:29:50.000000000Z", equity: "10069.97" }] }] },
    });
  });

  it("refuses an answer for a range other than the one asked, so no range is drawn as another", async () => {
    for (const range of HISTORY_RANGES.filter((r) => r !== "1w")) {
      const read = await served(historyRoute(range), historyRead(range), base);
      expect(read, range).toMatchObject({ ok: false, error: { decode: { path: "/range", problem: "not_canonical" } } });
    }
  });

  it("refuses points out of order or repeated, since each bucket gives one point oldest first", async () => {
    const swapped = apply(base, { label: "swapped", set: { "/accounts/0/points/0/at": "2026-10-08T14:29:50.000000000Z", "/accounts/0/points/1/at": "2026-10-07T20:59:50.000000000Z" } });
    expect(await served(historyRoute("1w"), historyRead("1w"), swapped)).toMatchObject({ ok: false, error: { decode: { path: "/accounts/0/points/1/at" } } });
    const repeated = apply(base, { label: "repeated", set: { "/accounts/0/points/1/at": "2026-10-07T20:59:50.000000000Z" } });
    expect(await served(historyRoute("1w"), historyRead("1w"), repeated)).toMatchObject({ ok: false });
  });

  it("refuses two points in one bucket of the range's width, and takes them in neighbouring buckets", async () => {
    const at = (first: string, second: string, range: HistoryRange) => apply(base, { label: range, set: { "/range": range, "/accounts/0/points/0/at": first, "/accounts/0/points/1/at": second } });
    const cases: Array<[HistoryRange, string, string, string]> = [
      // [range, first, second in the same bucket, second in the next bucket]
      ["1d", "2026-10-08T14:00:00.000000000Z", "2026-10-08T14:04:59.999999999Z", "2026-10-08T14:05:00.000000000Z"],
      ["1w", "2026-10-08T14:00:00.000000000Z", "2026-10-08T14:59:59.000000000Z", "2026-10-08T15:00:00.000000000Z"],
      ["1m", "2026-10-08T00:00:00.000000000Z", "2026-10-08T23:59:59.000000000Z", "2026-10-09T00:00:00.000000000Z"],
      ["all", "2026-10-08T00:00:01.000000000Z", "2026-10-08T12:00:00.000000000Z", "2026-10-09T00:00:00.000000000Z"],
    ];
    for (const [range, first, same, next] of cases) {
      expect(await served(historyRoute(range), historyRead(range), at(first, same, range)), `${range} same`).toMatchObject({ ok: false, error: { decode: { path: "/accounts/0/points/1/at", problem: "not_canonical" } } });
      expect(await served(historyRoute(range), historyRead(range), at(first, next, range)), `${range} next`).toMatchObject({ ok: true });
    }
  });

  it("refuses an account served twice", async () => {
    const body = { ...(structuredClone(base) as Record<string, unknown>) };
    const accounts = body.accounts as unknown[];
    body.accounts = [accounts[0], structuredClone(accounts[0])];
    expect(await served(historyRoute("1w"), historyRead("1w"), body)).toMatchObject({ ok: false, error: { decode: { path: "/accounts/1/account_ref", problem: "not_canonical" } } });
  });
});

describe("an agent's P&L", () => {
  const base = json("examples/read-models.pnl.json");
  const agent = "agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z";

  it("reads the gross figures as served, a basis not reported as null", async () => {
    expect(await served(pnlRoute(agent), pnlRead(agent), base)).toMatchObject({ ok: true, value: { agent_id: agent, account_ref: "acct_1", realized_gross: "25", unrealized_pnl: "30", instruments: [{ filled_qty: "6", realized_gross: "40", unrealized_pnl: "30" }, { filled_qty: "0", realized_gross: "-15", unrealized_pnl: "0" }] } });
    const unknown = apply(base, { label: "basis", set: { "/instruments/0/realized_gross": null, "/realized_gross": null } });
    expect(await served(pnlRoute(agent), pnlRead(agent), unknown)).toMatchObject({ ok: true, value: { realized_gross: null, instruments: [{ realized_gross: null }, {}] } });
  });

  it("refuses an answer naming another agent, so one agent's P&L is never shown as another's", async () => {
    expect(await served(pnlRoute("agt_other"), pnlRead("agt_other"), base)).toMatchObject({ ok: false, error: { decode: { path: "/agent_id", problem: "not_canonical" } } });
  });

  it("refuses a top-level figure that is not its instruments' exact sum, or not null when one of theirs is", async () => {
    const at = (set: Record<string, unknown>) => pnlRead(agent)(apply(base, { label: "sum", set }), "");
    expect(at({ "/realized_gross": "-3.5" })).toMatchObject({ ok: false, issue: { path: "/realized_gross", problem: "not_canonical" } });
    expect(at({ "/unrealized_pnl": "30.0000000000000000000000000001" })).toMatchObject({ ok: false, issue: { path: "/unrealized_pnl" } });
    expect(at({ "/realized_gross": null })).toMatchObject({ ok: false, issue: { path: "/realized_gross" } });
    expect(at({ "/instruments/1/unrealized_pnl": null })).toMatchObject({ ok: false, issue: { path: "/unrealized_pnl" } });
    // Sums at the decoder's full 28 places: 0.1 + 0.2 is 0.3 exactly, and a sum with no instrument is zero.
    expect(at({ "/instruments/0/realized_gross": "0.1", "/instruments/1/realized_gross": "0.2", "/realized_gross": "0.3" })).toMatchObject({ ok: true });
    expect(at({ "/instruments/0/unrealized_pnl": "0.0000000000000000000000000001", "/unrealized_pnl": "0.0000000000000000000000000001" })).toMatchObject({ ok: true });
    expect(at({ "/instruments": [], "/realized_gross": "0", "/unrealized_pnl": "0" })).toMatchObject({ ok: true });
    expect(at({ "/instruments": [], "/realized_gross": null, "/unrealized_pnl": "0" })).toMatchObject({ ok: false });
  });

  it("refuses a body that is not an object", async () => {
    for (const body of [[], "25", null]) expect(pnlRead(agent)(body, "")).toMatchObject({ ok: false });
  });
});

describe("the mock deployment serves the Portfolio reads from the fixtures (DEC-2413)", () => {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const client = createWorkspaceClient({ workspaceId: WS, fetch: server.fetch, newKey: () => "portfolio-key-000002" });

  it("serves the account's snapshot: the broker's equity, what no agent holds as cash, and the agents' day as its change", async () => {
    const read = await client.read(DASHBOARD_ROUTE, dashboardRead);
    expect(read).toMatchObject({ ok: true, value: { accounts: [{ snapshot: { equity: "28478.36", cash: "20045.59", buying_power: "20045.59", last_equity: "28422.8", day_change: "55.56", environment: "paper" } }] } });
    expect(read.ok && read.value.agents.map((a) => a.agent_id)).toEqual([AGENT_IDS.btc, AGENT_IDS.swing, AGENT_IDS.lmn]);
  });

  it("serves every range's history, one point per bucket oldest first, 1m without a query, and refuses another range", async () => {
    for (const range of HISTORY_RANGES) {
      const read = await client.read(historyRoute(range), historyRead(range));
      expect(read, range).toMatchObject({ ok: true, value: { range } });
      expect(read.ok && read.value.accounts[0].points.length, range).toBeGreaterThan(0);
    }
    expect(await client.read("/dashboard/history", historyRead("1m"))).toMatchObject({ ok: true, value: { range: "1m" } });
    expect(await client.read("/dashboard/history?range=1D", historyRead("1d"))).toMatchObject({ ok: false, error: { code: "invalid" } });
  });

  it("serves each agent's P&L with its sums, and not_found for an agent it does not know", async () => {
    const btc = await client.read(pnlRoute(AGENT_IDS.btc), pnlRead(AGENT_IDS.btc));
    expect(btc).toMatchObject({ ok: true, value: { realized_gross: "-24.56", unrealized_pnl: "148.01", instruments: [{ instrument_id: INSTRUMENTS.BTC.asset_id, filled_qty: "0.12" }] } });
    expect(await client.read(pnlRoute(AGENT_IDS.lmn), pnlRead(AGENT_IDS.lmn))).toMatchObject({ ok: true, value: { realized_gross: "10", unrealized_pnl: "0" } });
    expect(await client.read(pnlRoute("agt_nobody"), pnlRead("agt_nobody"))).toMatchObject({ ok: false, error: { code: "not_found" } });
  });
});
