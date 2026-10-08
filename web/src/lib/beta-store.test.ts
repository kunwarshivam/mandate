// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { POST } from "@/app/api/beta/route";
import type * as AuthConfig from "./auth-config";
import { LOCAL_FILE, localFileAllowed } from "./beta-store";

const supabase = vi.hoisted(() => ({ url: "", key: "", insertError: null as { code: string } | null, inserts: [] as unknown[] }));
const disk = vi.hoisted(() => ({ appendFile: vi.fn(async () => undefined), mkdir: vi.fn(async () => undefined) }));

vi.mock("node:fs/promises", () => ({ appendFile: disk.appendFile, mkdir: disk.mkdir }));
vi.mock("@/lib/auth-config", async (original) => ({
  ...(await original<typeof AuthConfig>()),
  get SUPABASE_URL() {
    return supabase.url;
  },
  get SUPABASE_PUBLISHABLE_KEY() {
    return supabase.key;
  },
}));
vi.mock("@supabase/supabase-js", () => ({
  createClient: () => ({
    from: (table: string) => ({
      insert: async (row: unknown) => {
        supabase.inserts.push({ table, row });
        return { error: supabase.insertError };
      },
    }),
  }),
}));

const post = (email: string) => POST(new Request("http://localhost:4317/api/beta", { method: "POST", body: JSON.stringify({ email, role: "own" }) }));

function configured(insertError: { code: string } | null = null) {
  supabase.url = "https://stub-project.supabase.co";
  supabase.key = "sb_publishable_stub";
  supabase.insertError = insertError;
}

beforeEach(() => {
  supabase.url = "";
  supabase.key = "";
  supabase.insertError = null;
  supabase.inserts = [];
  disk.appendFile.mockClear();
  disk.mkdir.mockClear();
});

afterEach(() => vi.unstubAllEnvs());

describe("the private beta store on Workers (DEC-823): the Supabase table in production, a local file only in development", () => {
  it.skip("pending E11-9: allows the local file in development and tests, and never in a production build", () => {
    expect(localFileAllowed("development")).toBe(true);
    expect(localFileAllowed("test")).toBe(true);
    for (const nodeEnv of ["production", undefined, "", "Production", "prod"]) expect(localFileAllowed(nodeEnv), String(nodeEnv)).toBe(false);
  });

  it.skip("pending E11-9: answers 503 unavailable in a production build with Supabase unset, and writes no file", async () => {
    expect(localFileAllowed("production")).toBe(false);
    vi.stubEnv("NODE_ENV", "production");
    const response = await post("ada@example.com");
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "unavailable" });
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(supabase.inserts).toEqual([]);
    expect(disk.mkdir).not.toHaveBeenCalled();
    expect(disk.appendFile).not.toHaveBeenCalled();
  });

  it.skip("pending E11-9: answers 503 unavailable in a production build whose Supabase has no beta_requests table, and writes no file", async () => {
    expect(localFileAllowed("production")).toBe(false);
    vi.stubEnv("NODE_ENV", "production");
    configured({ code: "PGRST205" });
    const response = await post("ada@example.com");
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "unavailable" });
    expect(supabase.inserts).toEqual([{ table: "beta_requests", row: { email: "ada@example.com", role: "own" } }]);
    expect(disk.appendFile).not.toHaveBeenCalled();
  });

  it.skip("pending E11-9: stores a request in the Supabase table in a production build, a repeat address included", async () => {
    expect(localFileAllowed("production")).toBe(false);
    vi.stubEnv("NODE_ENV", "production");
    for (const insertError of [null, { code: "23505" }]) {
      configured(insertError);
      const response = await post("ada@example.com");
      expect(response.status, String(insertError?.code)).toBe(200);
      expect(await response.json()).toEqual({ ok: true });
    }
    configured({ code: "08006" });
    expect((await post("ada@example.com")).status).toBe(503);
    expect(disk.appendFile).not.toHaveBeenCalled();
  });

  it.skip("pending E11-9: answers 503 in development when Supabase refuses the request for another reason, and writes no file", async () => {
    expect(localFileAllowed("development")).toBe(true);
    vi.stubEnv("NODE_ENV", "development");
    configured({ code: "08006" });
    const response = await post("ada@example.com");
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "unavailable" });
    expect(disk.appendFile).not.toHaveBeenCalled();
  });

  it.skip("pending E11-9: keeps the local file in development while Supabase is unset or has no table", async () => {
    expect(localFileAllowed("development")).toBe(true);
    vi.stubEnv("NODE_ENV", "development");
    expect((await post("ada@example.com")).status).toBe(200);
    configured({ code: "PGRST205" });
    expect((await post("bob@example.com")).status).toBe(200);
    expect(disk.appendFile).toHaveBeenCalledTimes(2);
    const lines = disk.appendFile.mock.calls.map((call) => call as unknown[]);
    expect(lines.map((call) => call[0])).toEqual([LOCAL_FILE, LOCAL_FILE]);
    expect(lines.map((call) => JSON.parse(String(call[1])).email)).toEqual(["ada@example.com", "bob@example.com"]);
  });
});
