import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { accountEquity, decodeConnection } from "./account";

function example(name: string): Record<string, unknown> {
  const examples = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "schemas", "workspace-api", "examples");
  return JSON.parse(readFileSync(join(examples, `read-models.${name}.json`), "utf8")) as Record<string, unknown>;
}

function withAccount(account: Record<string, unknown>) {
  const body = example("connection");
  const connection = structuredClone(body.connection) as Record<string, unknown>;
  connection.account = { ...(connection.account as Record<string, unknown>), ...account };
  return { ...body, connection };
}

function decoded(body: unknown) {
  const d = decodeConnection(body, "");
  if (!d.ok) throw new Error(`connection did not decode: ${JSON.stringify(d.issue)}`);
  return d.value;
}

describe("the account equity figure from GET /connections/{id} (§4.5)", () => {
  it.skip("pending E11-9: the schema's own example decodes and gives the figure with its freshness", () => {
    expect(accountEquity(decoded(example("connection")))).toEqual({
      connection_id: "con_01JB3K7M9Q2W4E6R8T0Y1V3X5P",
      environment: "paper",
      equity: "10412.37",
      freshness: { as_of: "2026-09-28T18:05:01.000000000Z", stale: false, age: "15 s" },
      account_state: "normal",
    });
  });

  it.skip("pending E11-9: a stale account figure is shown stale, never as current", () => {
    const view = accountEquity(
      decoded(withAccount({ freshness: { observed_at: "2026-09-28T17:50:20.000000000Z", stale: true, age_seconds: 900, limit_source: "account_snapshot.max_age_s" } })),
    );
    expect(view.freshness).toEqual({ as_of: "2026-09-28T17:50:20.000000000Z", stale: true, age: "15 min" });
  });

  it.skip("pending E11-9: a restricted account says so", () => {
    expect(accountEquity(decoded(withAccount({ state: "closing_only" }))).account_state).toBe("closing_only");
  });

  it.skip("pending E11-9: equity as a JSON number is refused (§3.1)", () => {
    expect(decodeConnection(withAccount({ equity: 10412.37 }), "")).toMatchObject({ ok: false, issue: { path: "/connection/account/equity", problem: "decimal_number" } });
  });

  it.skip("pending E11-9: an account state or broker outside the closed sets is refused", () => {
    expect(decodeConnection(withAccount({ state: "frozen" }), "").ok).toBe(false);
    const body = example("connection");
    expect(decodeConnection({ ...body, connection: { ...(body.connection as object), broker: "acme" } }, "").ok).toBe(false);
  });
});
