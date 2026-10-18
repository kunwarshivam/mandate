import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { POST } from "@/app/dev/stub-token/route.dev";
import { DEV_ROUTE_EXTENSION, pageExtensions } from "./dev-routes";
import { STUB_ISSUER_ENV, STUB_SUBJECT_ENV, STUB_TOKEN_PATH, mintStubToken, readStubSignIn, sameOriginAsk, tokenFromStub } from "./stub-sign-in";

const WEB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const ISSUER = "http://127.0.0.1:8090";
const env = (issuer: string, subject = "demo-owner") => ({ [STUB_ISSUER_ENV]: issuer, [STUB_SUBJECT_ENV]: subject });

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

describe("the stub sign-in's configuration (DEC-2406)", () => {
  it("is on only under next dev, with the stub's loopback issuer and a subject", () => {
    expect(readStubSignIn(env(ISSUER), "development")).toEqual({ issuer: ISSUER, subject: "demo-owner" });
    expect(readStubSignIn(env("http://[::1]:65535"), "development")?.issuer).toBe("http://[::1]:65535");
    for (const nodeEnv of ["production", "test", undefined]) expect(readStubSignIn(env(ISSUER), nodeEnv), String(nodeEnv)).toBeNull();
    expect(readStubSignIn({}, "development")).toBeNull();
  });

  it("refuses any issuer but an IP literal on loopback with a port, as DEC-2100 item 5 binds the stub", () => {
    for (const issuer of [
      "http://localhost:8090",
      "https://127.0.0.1:8090",
      "http://127.0.0.1",
      "http://127.0.0.1:0",
      "http://127.0.0.1:08090",
      "http://127.0.0.1:65536",
      "http://127.0.0.1:8090/",
      "http://10.0.0.1:8090",
      "http://user@127.0.0.1:8090",
    ])
      expect(readStubSignIn(env(issuer), "development"), issuer).toBeNull();
  });

  it("refuses a subject DEC-992 item 2 refuses: empty, with a control character, or over 1024 bytes", () => {
    for (const subject of ["", "demo\nowner", "demo\u0000", "é".repeat(513)]) expect(readStubSignIn(env(ISSUER, subject), "development"), JSON.stringify(subject)).toBeNull();
    expect(readStubSignIn(env(ISSUER, "a".repeat(1024)), "development")).not.toBeNull();
  });
});

describe("the token from the stub", () => {
  it("asks POST /token with the subject as JSON, and takes only a 200 with a string access_token", async () => {
    const asked: Array<{ url: string; init: RequestInit }> = [];
    const answer = (status: number, body: unknown) =>
      (async (url: string | URL | Request, init?: RequestInit) => {
        asked.push({ url: String(url), init: init ?? {} });
        return new Response(JSON.stringify(body), { status });
      }) as typeof fetch;
    const config = { issuer: ISSUER, subject: "demo-owner" };
    expect(await mintStubToken(config, answer(200, { access_token: "eyJ.stub", expires_in: 300 }))).toEqual({ ok: true, accessToken: "eyJ.stub" });
    expect(asked[0]).toMatchObject({ url: `${ISSUER}/token`, init: { method: "POST", headers: { "Content-Type": "application/json" }, body: '{"subject":"demo-owner"}' } });
    expect(await mintStubToken(config, answer(400, { access_token: "eyJ.stub" }))).toEqual({ ok: false });
    expect(await mintStubToken(config, answer(200, { access_token: 7 }))).toEqual({ ok: false });
    expect(await mintStubToken(config, answer(200, { access_token: "" }))).toEqual({ ok: false });
    expect(await mintStubToken(config, (() => Promise.reject(new TypeError("refused"))) as typeof fetch)).toEqual({ ok: false });
  });
});

describe("the app's route for it", () => {
  const ask = (headers: Record<string, string>) => new Request(`http://localhost:4317${STUB_TOKEN_PATH}`, { method: "POST", headers });
  const same = { host: "localhost:4317", origin: "https://localhost:4317", "x-mandate-request": "1" };

  it("answers the browser's same-origin ask with the token, never cached", async () => {
    vi.stubEnv("NODE_ENV", "development");
    vi.stubEnv(STUB_ISSUER_ENV, ISSUER);
    vi.stubEnv(STUB_SUBJECT_ENV, "demo-owner");
    vi.stubGlobal("fetch", async () => new Response(JSON.stringify({ access_token: "eyJ.stub", expires_in: 300 }), { status: 200 }));
    const answered = await POST(ask(same));
    expect(answered.status).toBe(200);
    expect(answered.headers.get("cache-control")).toBe("no-store");
    expect(await answered.json()).toEqual({ access_token: "eyJ.stub" });
  });

  it("is 404 when the stub sign-in is off, 403 to an ask without the mark or from another origin, and 502 when the stub does not answer", async () => {
    vi.stubEnv("NODE_ENV", "production");
    vi.stubEnv(STUB_ISSUER_ENV, ISSUER);
    vi.stubEnv(STUB_SUBJECT_ENV, "demo-owner");
    expect((await POST(ask(same))).status).toBe(404);
    vi.stubEnv("NODE_ENV", "development");
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("refused")));
    expect((await POST(ask({ ...same, "x-mandate-request": "0" }))).status).toBe(403);
    expect((await POST(ask({ ...same, origin: "https://evil.example" }))).status).toBe(403);
    expect((await POST(ask({ host: same.host, "x-mandate-request": "1" }))).status).toBe(403);
    expect((await POST(ask(same))).status).toBe(502);
  });

  it("checks the origin against the Host the browser sent", () => {
    expect(sameOriginAsk(new Headers({ host: "127.0.0.1:4317", origin: "http://127.0.0.1:4317", "x-mandate-request": "1" }))).toBe(true);
    expect(sameOriginAsk(new Headers({ host: "127.0.0.1:4317", origin: "http://127.0.0.1:4318", "x-mandate-request": "1" }))).toBe(false);
    expect(sameOriginAsk(new Headers({ host: "127.0.0.1:4317", origin: "null", "x-mandate-request": "1" }))).toBe(false);
  });

  it("is a dev route a production build does not compile, and that its middleware refuses outside next dev", () => {
    expect(pageExtensions("production")).not.toContain(DEV_ROUTE_EXTENSION);
    expect(pageExtensions("development")).toContain(DEV_ROUTE_EXTENSION);
    expect(readdirSync(join(WEB, "src/app/dev/stub-token"))).toEqual(["route.dev.ts"]);
    expect(readFileSync(join(WEB, "src/middleware.ts"), "utf8")).toContain('if (process.env.NODE_ENV !== "development" && DEV_ONLY.test(request.nextUrl.pathname)) return new NextResponse(null, { status: 404 });');
  });
});

describe("the browser's token from that route", () => {
  const answering = (status: number, body?: unknown) => async (input: string, init: RequestInit) => {
    expect(input).toBe(STUB_TOKEN_PATH);
    expect(init).toMatchObject({ method: "POST", credentials: "same-origin", headers: { "X-Mandate-Request": "1" } });
    return new Response(body === undefined ? null : JSON.stringify(body), { status });
  };

  it("is the token, none when the route is off or refuses, and unreachable when the stub or the route does not answer", async () => {
    expect(await tokenFromStub(answering(200, { access_token: "eyJ.stub" }))).toBe("eyJ.stub");
    expect(await tokenFromStub(answering(404))).toBeNull();
    expect(await tokenFromStub(answering(403))).toBeNull();
    expect(await tokenFromStub(answering(502))).toBe("unreachable");
    expect(await tokenFromStub(answering(200, { token: "eyJ.stub" }))).toBe("unreachable");
    expect(await tokenFromStub(() => Promise.reject(new TypeError("offline")))).toBe("unreachable");
  });
});
