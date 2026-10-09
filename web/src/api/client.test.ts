import { afterEach, describe, expect, it, vi } from "vitest";
import { CSRF_HEADER, OPERATIONS, createWorkspaceClient, isIdempotencyKey, newIdempotencyKey, type ClientOptions, type Fetch, type ProblemFields } from "./client";
import { decimal, object, closedEnum } from "./decode";
import type { KillSwitchRequest, Operation } from "./types";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const HASH = `sha256:${"0f".repeat(32)}` as const;
const MARK = { stream_id: "control", seq: 7, hash: HASH, recorded_at: "2026-09-28T18:05:20.000000000Z" };
const EVENT = "01J9ZQ4M5KQ3W8X2Y7V6T5R4S3";

interface Call {
  url: string;
  credentials: RequestCredentials | undefined;
  method: string;
  headers: Headers;
  body: string | null;
}

function json(status: number, body: unknown, type = "application/json"): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": type } });
}

function problem(status: number, members: Record<string, unknown>): Response {
  return json(status, { type: "about:blank", status, title: "Request refused", ...members }, "application/problem+json");
}

function fake(answer: (call: Call) => Response | Promise<Response>) {
  const calls: Call[] = [];
  const fetch: Fetch = async (input, init) => {
    const call = { url: input, credentials: init.credentials, method: init.method ?? "GET", headers: new Headers(init.headers), body: typeof init.body === "string" ? init.body : null };
    calls.push(call);
    return answer(call);
  };
  return { fetch, calls };
}

let keys = 0;
const newKey = () => `test-key-${String(++keys).padStart(8, "0")}`;

function client(answer: (call: Call) => Response | Promise<Response>, extra: Pick<ClientOptions, "onUnauthenticated" | "schema"> = {}) {
  const f = fake(answer);
  return { api: createWorkspaceClient({ workspaceId: WS, fetch: f.fetch, newKey, ...extra }), calls: f.calls };
}

const accepted = () => json(202, { command_id: EVENT, phase: "recorded", step_up_status: "missing" });
const PARAMS = { agent: "agt_01", approval: "apr_01", delegation: "dlg_01", version: HASH };
const ALL = Object.keys(OPERATIONS) as Operation[];

const equity = object<{ equity: string; mode: "normal" | "exits_only" | "paused" | "stopped" }>({
  equity: decimal,
  mode: closedEnum(["normal", "exits_only", "paused", "stopped"]),
});

const workspaceKill: KillSwitchRequest = { scope: { kind: "workspace", id: null }, environment_shown: "paper", owner_exit: null, record: null, step_up: null };

describe("paths (spec §4)", () => {
  it("pending E11-9: puts every route under /v1/workspaces/{ws}", async () => {
    const { api, calls } = client(() => json(200, { api_version: "1", as_of: [MARK], equity: "1", mode: "normal" }));
    expect(api.path("/health")).toBe(`/v1/workspaces/${WS}/health`);
    await api.read("/dashboard", equity);
    expect(calls[0]).toMatchObject({ url: `/v1/workspaces/${WS}/dashboard`, method: "GET" });
    for (const operation of ALL) {
      expect(api.prepare(operation, PARAMS, {}).path.startsWith(`/v1/workspaces/${WS}/`)).toBe(true);
    }
  });

  it("pending E11-9: fills route parameters, encoded, and refuses a missing one", () => {
    const { api } = client(accepted);
    expect(api.prepare("pause", { agent: "agt/../x" }, {}).path).toBe(`/v1/workspaces/${WS}/agents/agt%2F..%2Fx/pause`);
    expect(() => api.prepare("pause", {}, {})).toThrow();
    for (const agent of ["", ".", ".."]) expect(() => api.prepare("pause", { agent }, {}), agent).toThrow();
    for (const workspaceId of ["", "..", "../other", "ws/x", "ws?x"]) {
      expect(() => createWorkspaceClient({ workspaceId, fetch: fake(accepted).fetch }), workspaceId).toThrow();
    }
  });
});

describe("idempotency (spec §3.4, API-4)", () => {
  it("pending E11-9: accepts keys of 16 to 64 characters from [A-Za-z0-9_-] only", () => {
    expect(isIdempotencyKey("a".repeat(16))).toBe(true);
    expect(isIdempotencyKey("A-z_9".repeat(12) + "abcd")).toBe(true);
    expect(isIdempotencyKey("a".repeat(15))).toBe(false);
    expect(isIdempotencyKey("a".repeat(65))).toBe(false);
    expect(isIdempotencyKey("a".repeat(15) + "+")).toBe(false);
    expect(isIdempotencyKey("a".repeat(15) + "é")).toBe(false);
  });

  it("pending E11-9: makes a fresh valid key per gesture", () => {
    const made = Array.from({ length: 200 }, newIdempotencyKey);
    expect(made.every(isIdempotencyKey)).toBe(true);
    expect(new Set(made).size).toBe(made.length);
  });

  it("pending E11-9: sends an Idempotency-Key on every mutating call", async () => {
    const { api, calls } = client(accepted);
    for (const operation of ALL) await api.send(api.prepare(operation, PARAMS, {}));
    await api.send(api.prepareKillSwitch(workspaceKill));
    expect(calls).toHaveLength(ALL.length + 1);
    for (const call of calls) {
      expect(call.method).toBe("POST");
      expect(isIdempotencyKey(call.headers.get("Idempotency-Key") ?? "")).toBe(true);
      expect(call.headers.get("Content-Type")).toBe("application/json");
    }
    expect(new Set(calls.map((c) => c.headers.get("Idempotency-Key"))).size).toBe(calls.length);
  });

  it("pending E11-9: keeps the key, path and body of one gesture across retries", async () => {
    let first = true;
    const { api, calls } = client(() => {
      if (first) {
        first = false;
        throw new TypeError("network");
      }
      return accepted();
    });
    const command = api.prepare("pause", PARAMS, { record: null });
    const lost = await api.send(command);
    expect(lost.ok).toBe(false);
    if (lost.ok) return;
    expect(lost.command).toBe(command);
    expect((await api.send(lost.command)).ok).toBe(true);
    expect(calls[1].headers.get("Idempotency-Key")).toBe(calls[0].headers.get("Idempotency-Key"));
    expect(calls[1].url).toBe(calls[0].url);
    expect(calls[1].body).toBe(calls[0].body);
  });

  it("pending E11-9: freezes a prepared command so a retry cannot change it", () => {
    const { api } = client(accepted);
    const command = api.prepare("pause", PARAMS, { record: null });
    expect(Object.isFrozen(command)).toBe(true);
  });
});

describe("command outcomes (spec §3.5, §5, API-13)", () => {
  it("pending E11-9: reports recorded only with the event that recorded it", async () => {
    const { api } = client(accepted);
    expect(await api.send(api.prepare("pause", PARAMS, {}))).toEqual({
      ok: true,
      effect: "recorded",
      accepted: { event_id: EVENT, phase: "recorded", step_up_status: "missing" },
    });
    const responses = client(() => json(202, { response_id: EVENT, phase: "recorded" }));
    const answered = await responses.api.send(responses.api.prepare("approval_response", PARAMS, {}));
    expect(answered).toMatchObject({ ok: true, accepted: { event_id: EVENT, step_up_status: null } });
  });

  it("pending E11-9: surfaces a network failure as result unknown, never success and never nothing happened", async () => {
    const { api } = client(() => {
      throw new TypeError("Failed to fetch");
    });
    const outcome = await api.send(api.prepare("stop", PARAMS, {}));
    expect(outcome).toMatchObject({ ok: false, error: { code: "network", effect: "unknown", status: null, event_id: null, retryable: true } });
  });

  it("pending E11-9: keeps the event id of an ambiguous append", async () => {
    const { api } = client(() => problem(503, { code: "append_ambiguous", effect: "unknown", event_id: EVENT, retryable: true }));
    const outcome = await api.send(api.prepare("pause", PARAMS, {}));
    expect(outcome).toMatchObject({ ok: false, error: { code: "append_ambiguous", effect: "unknown", event_id: EVENT, status: 503 } });
  });

  it("pending E11-9: decodes a problem document to code and effect, keeping an unknown code", async () => {
    const violations = [{ path: "/scope/id", code: "invalid", message: "Not an id" }];
    const cases = [
      { status: 409, code: "idempotency_conflict", effect: "none", retryable: false, event_id: null as string | null },
      { status: 422, code: "invalid", effect: "none", retryable: false, violations },
      { status: 503, code: "journal_unavailable", effect: "none", retryable: true },
      { status: 401, code: "step_up_required", effect: "none", retryable: false },
      { status: 409, code: "added_in_v1_3", effect: "none", retryable: false },
    ];
    for (const c of cases) {
      const { api } = client(() => problem(c.status, c));
      const outcome = await api.send(api.prepare("resume", PARAMS, {}));
      expect(outcome, c.code).toMatchObject({
        ok: false,
        error: { code: c.code, effect: c.effect, status: c.status, retryable: c.retryable, event_id: c.event_id ?? null, violations: c.violations ?? [] },
      });
    }
  });

  it("pending E11-9: never reads nothing happened into an answer that does not say so", async () => {
    const answers = [
      () => problem(500, { code: "internal" }),
      () => problem(500, { code: "internal", effect: "maybe" }),
      () => new Response("<html>Bad gateway</html>", { status: 502, headers: { "Content-Type": "text/html" } }),
      () => json(202, { phase: "recorded" }),
      () => json(202, { command_id: EVENT, phase: "applied_somehow" }),
      () => new Response("not json", { status: 202 }),
    ];
    for (const answer of answers) {
      const { api } = client(answer);
      const outcome = await api.send(api.prepare("pause", PARAMS, {}));
      expect(outcome.ok).toBe(false);
      if (!outcome.ok) expect(outcome.error.effect).toBe("unknown");
    }
  });

  it("pending E11-9: makes an unknown closed-enum value in an answer a typed error", async () => {
    const { api } = client(() => json(202, { command_id: EVENT, phase: "recorded", step_up_status: "half_bound" }));
    const outcome = await api.send(api.prepare("pause", PARAMS, {}));
    expect(outcome).toMatchObject({
      ok: false,
      error: { code: "invalid_response", effect: "unknown", event_id: EVENT, decode: { path: "/step_up_status", problem: "unknown_enum_value", value: "half_bound" } },
    });
  });
});

describe("reads (spec §3.2, §6.1, API-14)", () => {
  it("pending E11-9: keeps the as_of watermarks and api_version of every response", async () => {
    const second = { ...MARK, stream_id: "agent:agt_01", seq: 900 };
    const { api } = client(() => json(200, { api_version: "1.0", build: HASH, as_of: [MARK, second], equity: "100.25", mode: "paused" }));
    expect(await api.read("/agents/agt_01", equity)).toEqual({ ok: true, value: { equity: "100.25", mode: "paused" }, as_of: [MARK, second], api_version: "1.0" });
  });

  it("pending E11-9: ignores response members it does not know", async () => {
    const { api } = client(() => json(200, { api_version: "1", as_of: [MARK], equity: "1", mode: "normal", added_later: [1, 2], nested: { x: 1 } }));
    expect(await api.read("/dashboard", equity)).toMatchObject({ ok: true, value: { equity: "1", mode: "normal" } });
  });

  it("pending E11-9: refuses a response without valid watermarks", async () => {
    for (const body of [{ api_version: "1", equity: "1", mode: "normal" }, { api_version: "1", as_of: [{ ...MARK, seq: -1 }], equity: "1", mode: "normal" }]) {
      const { api } = client(() => json(200, body));
      expect(await api.read("/dashboard", equity)).toMatchObject({ ok: false, error: { code: "invalid_response", effect: "none" } });
    }
  });

  it("pending E11-9: refuses a JSON number in a decimal member of a response", async () => {
    const { api } = client(() => json(200, { api_version: "1", as_of: [MARK], equity: 100.25, mode: "normal" }));
    expect(await api.read("/dashboard", equity)).toMatchObject({ ok: false, error: { code: "invalid_response", decode: { path: "/equity", problem: "decimal_number" } } });
  });

  it("pending E11-9: makes an unknown closed-enum value a typed error the screen can render", async () => {
    const { api } = client(() => json(200, { api_version: "1", as_of: [MARK], equity: "1", mode: "halted" }));
    expect(await api.read("/dashboard", equity)).toMatchObject({
      ok: false,
      error: { code: "invalid_response", effect: "none", decode: { path: "/mode", problem: "unknown_enum_value", value: "halted", allowed: ["normal", "exits_only", "paused", "stopped"] } },
    });
  });

  it("pending E11-9: says a read that got no answer changed nothing", async () => {
    const { api } = client(() => {
      throw new TypeError("Failed to fetch");
    });
    expect(await api.read("/dashboard", equity)).toMatchObject({ ok: false, error: { code: "network", effect: "none", status: null } });
  });

  it("pending E11-9: decodes command status with its steps and watermarks (spec §5.5)", async () => {
    const step = { stream_id: "agent:agt_01", seq: 12, event_type: "AgentModeChanged", recorded_at: MARK.recorded_at, reason: null };
    const { api, calls } = client(() => json(200, { api_version: "1", as_of: [MARK], phase: "applied", steps: [step] }));
    expect(await api.commandStatus(EVENT)).toEqual({ ok: true, value: { phase: "applied", steps: [step] }, as_of: [MARK], api_version: "1" });
    expect(calls[0].url).toBe(`/v1/workspaces/${WS}/commands/${EVENT}`);
    const odd = client(() => json(200, { api_version: "1", as_of: [MARK], phase: "done", steps: [] }));
    expect(await odd.api.commandStatus(EVENT)).toMatchObject({ ok: false, error: { decode: { problem: "unknown_enum_value", path: "/phase" } } });
  });
});

describe("authentication (spec §3.3, §3.5)", () => {
  it("pending E11-9: decodes a 401 to unauthenticated and tells the app once", async () => {
    const onUnauthenticated = vi.fn();
    const { api } = client(() => problem(401, { code: "unauthenticated", effect: "none", retryable: false }), { onUnauthenticated });
    expect(await api.read("/dashboard", equity)).toMatchObject({ ok: false, error: { code: "unauthenticated", effect: "none", status: 401 } });
    expect(onUnauthenticated).toHaveBeenCalledTimes(1);
  });

  it("pending E11-9: treats a bare 401 as unauthenticated", async () => {
    const onUnauthenticated = vi.fn();
    const { api } = client(() => new Response(null, { status: 401 }), { onUnauthenticated });
    expect(await api.read("/dashboard", equity)).toMatchObject({ ok: false, error: { code: "unauthenticated", status: 401 } });
    expect(onUnauthenticated).toHaveBeenCalledTimes(1);
  });

  it("pending E11-9: decodes a 401 on a command to unauthenticated with effect none and tells the app once", async () => {
    const onUnauthenticated = vi.fn();
    const { api } = client(() => problem(401, { code: "unauthenticated", effect: "none", retryable: false }), { onUnauthenticated });
    expect(await api.send(api.prepareKillSwitch(workspaceKill))).toMatchObject({ ok: false, error: { code: "unauthenticated", effect: "none", status: 401 } });
    expect(onUnauthenticated).toHaveBeenCalledTimes(1);
  });

  it("pending E11-9: never signs the owner out for step_up_required", async () => {
    const onUnauthenticated = vi.fn();
    const { api } = client(() => problem(401, { code: "step_up_required", effect: "none", retryable: false }), { onUnauthenticated });
    expect(await api.send(api.prepare("resume", PARAMS, {}))).toMatchObject({ ok: false, error: { code: "step_up_required", effect: "none" } });
    expect(onUnauthenticated).not.toHaveBeenCalled();
  });

  it("pending E11-9: sends the session cookie and no bearer token", async () => {
    const { api, calls } = client(accepted);
    await api.send(api.prepare("pause", PARAMS, {}));
    await api.read("/dashboard", equity);
    expect(calls.every((c) => c.headers.get("Authorization") === null)).toBe(true);
    expect(calls.map((c) => `${c.method} ${c.credentials}`)).toEqual(["POST same-origin", "GET same-origin"]);
  });

  it("pending E11-9: sends X-Mandate-Request: 1 on every command, the kill switch included (spec §3.3, DEC-681)", async () => {
    const { api, calls } = client(accepted);
    for (const operation of ALL) await api.send(api.prepare(operation, PARAMS, {}));
    await api.send(api.prepareKillSwitch(workspaceKill));
    expect(CSRF_HEADER).toBe("X-Mandate-Request");
    expect(calls.map((c) => c.headers.get("X-Mandate-Request"))).toEqual(calls.map(() => "1"));
  });

  it("pending E11-9: decodes 403 forbidden with effect none as nothing recorded, never unknown", async () => {
    const { api } = client(() => problem(403, { code: "forbidden", effect: "none", retryable: false }));
    expect(await api.send(api.prepareKillSwitch(workspaceKill))).toMatchObject({ ok: false, error: { code: "forbidden", effect: "none", status: 403, retryable: false } });
  });
});

describe("no order ticket (DEC-528)", () => {
  const ORDERISH = /order|buy|sell|trade|place|ticket|request|submit|execute|fill|position/i;
  /**
   * Every module in src/api/ and every name it exports. An implementation that adds or removes a
   * module or an export edits this list, and only this list, beyond un-skipping its pending tests (DEC-750 item 4).
   */
  const EXPORTS: Record<string, string[]> = {
    client: ["CSRF_HEADER", "OPERATIONS", "createWorkspaceClient", "isIdempotencyKey", "newIdempotencyKey"],
    decode: ["array", "closedEnum", "contentRef", "decimal", "integer", "isRecord", "nullable", "object", "string", "timestamp", "watermark"],
    "mock-server": ["createMockServer"],
    types: ["COMMAND_PHASES", "EFFECTS", "STEP_UP_STATUSES"],
  };
  const MODULES = Object.fromEntries(
    Object.entries(import.meta.glob(["./*.ts", "!./*.test.ts"], { eager: true }) as Record<string, Record<string, unknown>>).map(([path, module]) => [path.replace(/^\.\/(.*)\.ts$/, "$1"), module]),
  );

  it("has exactly the allowed modules in src/api, each exporting exactly its allowed names, none of them order-placing", () => {
    expect(Object.keys(MODULES).sort()).toEqual(Object.keys(EXPORTS).sort());
    for (const [name, module] of Object.entries(MODULES)) {
      const exported = Object.keys(module).sort();
      expect(exported, name).toEqual(EXPORTS[name]);
      expect(exported.filter((e) => ORDERISH.test(e)), name).toEqual([]);
    }
  });

  it("pending E11-9: exposes no order-placing method or operation", () => {
    const { api } = client(accepted);
    const members = new Set<string>();
    for (let o: object | null = api; o && o !== Object.prototype; o = Object.getPrototypeOf(o)) {
      for (const name of Object.getOwnPropertyNames(o)) members.add(name);
    }
    expect([...members].filter((name) => ORDERISH.test(name))).toEqual([]);
    expect(ALL.filter((op) => ORDERISH.test(op))).toEqual([]);
    expect(Object.values(OPERATIONS).filter((route) => /\/(orders|requests|positions|fills|trades)\b/.test(route))).toEqual([]);
  });

  it("pending E11-9: refuses an operation outside its table at runtime", () => {
    const { api } = client(accepted);
    expect(() => api.prepare("place_order" as Operation, PARAMS, {})).toThrow();
  });
});

describe("the kill switch (spec §5.4, rule 13)", () => {
  it("pending E11-9: sends exactly the request it was given, live and with a record", async () => {
    const { api, calls } = client(accepted);
    const record = { artifact: `sha256:${"aa".repeat(32)}`, ui_build: `sha256:${"bb".repeat(32)}` } as const;
    await api.send(api.prepareKillSwitch({ scope: { kind: "connection", id: "con_01" }, environment_shown: "live", owner_exit: null, record, step_up: null }));
    expect(calls[0].body).toBe(JSON.stringify({ scope: { kind: "connection", id: "con_01" }, environment_shown: "live", owner_exit: null, record, step_up: null }));
  });

  it("pending E11-9: sends with record null before any read has loaded", async () => {
    const { api, calls } = client((call) => {
      if (call.method !== "POST") throw new TypeError("reads are down");
      return accepted();
    });
    const outcome = await api.send(api.prepareKillSwitch(workspaceKill));
    expect(outcome).toMatchObject({ ok: true, effect: "recorded", accepted: { event_id: EVENT } });
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toBe(`/v1/workspaces/${WS}/kill-switch`);
    expect(JSON.parse(calls[0].body ?? "")).toEqual({ scope: { kind: "workspace", id: null }, environment_shown: "paper", owner_exit: null, record: null, step_up: null });
  });

  it("pending E11-9: names an agent scope and keeps its key across a retry", async () => {
    let n = 0;
    const { api, calls } = client(() => {
      n += 1;
      if (n === 1) throw new TypeError("network");
      return accepted();
    });
    const command = api.prepareKillSwitch({ ...workspaceKill, scope: { kind: "agent", id: "agt_01" } });
    expect(command.operation).toBe("kill_switch");
    const first = await api.send(command);
    expect(first).toMatchObject({ ok: false, error: { effect: "unknown" } });
    await api.send(command);
    expect(calls[1].headers.get("Idempotency-Key")).toBe(calls[0].headers.get("Idempotency-Key"));
    expect(JSON.parse(calls[1].body ?? "").scope).toEqual({ kind: "agent", id: "agt_01" });
  });
});

describe("idempotency keys come from the CSPRNG (spec §3.4)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("pending E11-9: draws every key from crypto.getRandomValues", () => {
    const random = vi.spyOn(globalThis.crypto, "getRandomValues").mockImplementation((array) => {
      if (array instanceof Uint8Array) array.fill(0);
      return array;
    });
    const mathRandom = vi.spyOn(Math, "random");
    const a = newIdempotencyKey();
    const b = newIdempotencyKey();
    expect(random).toHaveBeenCalled();
    expect(mathRandom).not.toHaveBeenCalled();
    expect(isIdempotencyKey(a)).toBe(true);
    expect(b).toBe(a);
  });
});

describe("problem defaults (spec §3.5)", () => {
  it("pending E11-9: defaults retryable from the effect when the problem omits it", async () => {
    const cases = [
      { effect: "none", retryable: false },
      { effect: "recorded", retryable: false },
      { effect: "unknown", retryable: true },
    ];
    for (const c of cases) {
      const { api } = client(() => problem(409, { code: "some_code", effect: c.effect }));
      expect(await api.send(api.prepare("pause", PARAMS, {})), c.effect).toMatchObject({ ok: false, error: { effect: c.effect, retryable: c.retryable } });
    }
  });
});

describe("the common-schema seam", () => {
  const conflict = () => problem(409, { code: "idempotency_conflict", effect: "none", retryable: false });
  const stated = (fields: Partial<ProblemFields>): ProblemFields => ({ code: null, effect: null, event_id: null, retryable: null, violations: [], ...fields });

  it("pending E11-9: makes a command unknown when a custom problem decoder states no effect", async () => {
    const { api } = client(conflict, { schema: { problem: () => stated({ code: "idempotency_conflict" }) } });
    expect(await api.send(api.prepare("pause", PARAMS, {}))).toMatchObject({ ok: false, error: { code: "idempotency_conflict", effect: "unknown" } });
  });

  it("pending E11-9: makes a command unknown, never a rejection, when a custom decoder throws", async () => {
    const throwing = () => {
      throw new Error("generated decoder failed");
    };
    const failing = client(conflict, { schema: { problem: throwing } });
    await expect(failing.api.send(failing.api.prepare("pause", PARAMS, {}))).resolves.toMatchObject({ ok: false, error: { effect: "unknown" } });
    const reads = client(() => json(200, { api_version: "1", as_of: [MARK], equity: "1", mode: "normal" }), { schema: { envelope: throwing } });
    await expect(reads.api.read("/dashboard", equity)).resolves.toMatchObject({ ok: false, error: { code: "invalid_response", effect: "none" } });
  });

  it("pending E11-9: keeps a default when the schema leaves a member undefined", async () => {
    const { api } = client(conflict, { schema: { problem: undefined, envelope: undefined } });
    expect(await api.send(api.prepare("pause", PARAMS, {}))).toMatchObject({ ok: false, error: { code: "idempotency_conflict", effect: "none" } });
  });
});
