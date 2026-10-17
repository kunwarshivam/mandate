import { describe, expect, it, vi } from "vitest";
import type { Fetch } from "./client";
import { type AccessToken, type Locks, REFRESH_EVERY_MS, SESSION_LOCK, createSessionKeeper } from "./session";

const ORIGIN = "https://api.owlhead.ai";

interface Call {
  method: string;
  url: string;
  body: unknown;
  headers: Headers;
  credentials: RequestCredentials | undefined;
}

/** A reader-writer lock, as `navigator.locks` grants one name: many shared holders or one exclusive. */
function fakeLocks(): Locks & { names: string[]; modes: string[] } {
  let shared = 0;
  let exclusive = false;
  const waiting: Array<() => void> = [];
  const wake = () => waiting.splice(0).forEach((w) => w());
  const names: string[] = [];
  const modes: string[] = [];
  return {
    names,
    modes,
    async request(name, { mode }, run) {
      names.push(name);
      modes.push(mode);
      const free = () => (mode === "shared" ? !exclusive : !exclusive && shared === 0);
      while (!free()) await new Promise<void>((r) => waiting.push(r));
      if (mode === "shared") shared += 1;
      else exclusive = true;
      try {
        return await run();
      } finally {
        if (mode === "shared") shared -= 1;
        else exclusive = false;
        wake();
      }
    },
  };
}

function server(answer: (call: Call) => Response | Promise<Response> = () => new Response(null, { status: 204 })) {
  const calls: Call[] = [];
  const fetch: Fetch = async (input, init) => {
    const call = { method: init.method ?? "GET", url: input, body: typeof init.body === "string" ? JSON.parse(init.body) : undefined, headers: new Headers(init.headers), credentials: init.credentials };
    calls.push(call);
    return answer(call);
  };
  return { fetch, calls };
}

function keeper(token: AccessToken, answer?: Parameters<typeof server>[0], extra: { locks?: Locks; latest?: number; now?: number; deaf?: boolean } = {}) {
  const { fetch, calls } = server(answer);
  let latest = extra.latest ?? 0;
  const posted: number[] = [];
  const session = createSessionKeeper({
    origin: ORIGIN,
    fetch,
    accessToken: async () => token,
    locks: extra.locks,
    channel: { post: (at) => (posted.push(at), extra.deaf ? latest : (latest = at)), latest: () => latest },
    now: () => extra.now ?? 1_000_000,
  });
  return { session, calls, posted };
}

describe("the API session (workspace API spec §3.11 items 5 and 6)", () => {
  it("opens with the sign-in client's access token, as a browser-only call with the API's cookie", async () => {
    const { session, calls } = keeper("eyJ.access.token");
    expect(await session.open()).toBe("open");
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({ method: "POST", url: `${ORIGIN}/v1/sessions`, body: { access_token: "eyJ.access.token" }, credentials: "include" });
    expect(calls[0].headers.get("X-Mandate-Request")).toBe("1");
    expect(calls[0].headers.get("Content-Type")).toBe("application/json");
    expect(calls[0].headers.has("Authorization")).toBe(false);
  });

  it("is refused on the one 401 or a 403, unanswered on any other status, and sends nothing without a token", async () => {
    for (const status of [401, 403]) expect(await keeper("t", () => new Response(null, { status })).session.open(), String(status)).toBe("refused");
    for (const status of [200, 429, 500, 503]) expect(await keeper("t", () => new Response(null, { status })).session.open(), String(status)).toBe("unreachable");
    const signedOut = keeper(null);
    expect(await signedOut.session.open()).toBe("signed_out");
    expect(signedOut.calls).toHaveLength(0);
    const down = keeper("unreachable");
    expect(await down.session.open()).toBe("unreachable");
    expect(down.calls).toHaveLength(0);
    expect(await keeper("t", () => Promise.reject(new TypeError("offline"))).session.open()).toBe("unreachable");
  });

  it("refreshes with a fresh token, or with null on a provider outage, which keeps only pause and the kill switch", async () => {
    const granted = keeper("fresh");
    expect(await granted.session.refresh()).toBe("open");
    expect(granted.calls[0]).toMatchObject({ method: "POST", url: `${ORIGIN}/v1/sessions/refresh`, body: { access_token: "fresh" }, credentials: "include" });
    expect(granted.calls[0].headers.get("X-Mandate-Request")).toBe("1");
    expect(granted.posted).toEqual([1_000_000]);
    const outage = keeper("unreachable");
    expect(await outage.session.refresh()).toBe("outage");
    expect(outage.calls[0].body).toEqual({ access_token: null });
    expect(await keeper("bad", () => new Response(null, { status: 401 })).session.refresh()).toBe("refused");
  });

  it("does not refresh again when another tab refreshed less than half a lifetime ago", async () => {
    const { session, calls } = keeper("fresh", undefined, { latest: 1_000_000 - REFRESH_EVERY_MS / 2 + 1 });
    expect(await session.refresh()).toBe("open");
    expect(calls).toHaveLength(0);
  });

  it("serializes refreshes under one exclusive lock, so a second never starts before the first's rotated cookie is set", async () => {
    const locks = fakeLocks();
    let inFlight = 0;
    let most = 0;
    const releases: Array<() => void> = [];
    const answer = async () => {
      inFlight += 1;
      most = Math.max(most, inFlight);
      await new Promise<void>((r) => releases.push(r));
      inFlight -= 1;
      return new Response(null, { status: 204 });
    };
    const { session, calls } = keeper("fresh", answer, { locks, deaf: true });
    const first = session.refresh();
    const second = session.refresh();
    await vi.waitFor(() => expect(releases).toHaveLength(1));
    await new Promise((r) => setTimeout(r, 5));
    expect(calls).toHaveLength(1);
    releases[0]();
    await vi.waitFor(() => expect(releases).toHaveLength(2));
    releases[1]();
    expect(await Promise.all([first, second])).toEqual(["open", "open"]);
    expect(calls).toHaveLength(2);
    expect(most).toBe(1);
  });

  it("takes the lock exclusive to open, refresh and sign out, and shared to read", async () => {
    const locks = fakeLocks();
    const { session } = keeper("fresh", undefined, { locks, deaf: true });
    await session.open();
    await session.refresh();
    await session.signOut();
    await session.guard(async () => null);
    expect(locks.names.every((n) => n === SESSION_LOCK)).toBe(true);
    expect(locks.modes).toEqual(["exclusive", "exclusive", "exclusive", "shared"]);
  });

  it("holds reads back while a refresh is in flight, and lets them through together otherwise", async () => {
    const locks = fakeLocks();
    let release: () => void = () => {};
    const slow = new Promise<void>((r) => (release = r));
    const order: string[] = [];
    const { session } = keeper("fresh", async () => (await slow, order.push("refreshed"), new Response(null, { status: 204 })), { locks });
    const refreshing = session.refresh();
    await Promise.resolve();
    const read = session.guard(async () => order.push("read"));
    await new Promise((r) => setTimeout(r, 5));
    expect(order).toEqual([]);
    release();
    await Promise.all([refreshing, read]);
    expect(order).toEqual(["refreshed", "read"]);
    const both = await Promise.all([session.guard(async () => 1), session.guard(async () => 2)]);
    expect(both).toEqual([1, 2]);
  });

  it("signs out by ending the current session", async () => {
    const { session, calls } = keeper("t");
    expect(await session.signOut()).toBe("signed_out");
    expect(calls[0]).toMatchObject({ method: "DELETE", url: `${ORIGIN}/v1/sessions/current`, credentials: "include" });
    expect(calls[0].headers.get("X-Mandate-Request")).toBe("1");
  });

  it("keeps the token out of browser storage and out of every URL", async () => {
    const stored = vi.spyOn(Storage.prototype, "setItem");
    const { session, calls } = keeper("secret.token.value");
    await session.open();
    await session.refresh();
    expect(stored).not.toHaveBeenCalled();
    for (const call of calls) expect(call.url).not.toContain("secret.token.value");
    stored.mockRestore();
  });
});
