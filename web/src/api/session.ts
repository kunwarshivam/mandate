/**
 * The browser's session with the workspace API (workspace API spec §3.11 items 5 and 6; DEC-1095).
 * The app trades the sign-in client's access token for the API's own session cookie with
 * `POST /v1/sessions`, refreshes it with `POST /v1/sessions/refresh` before its five-minute access
 * lifetime ends, and ends it with `DELETE /v1/sessions/current`. A refresh rotates the cookie's
 * secret and a rotated secret presented again ends the session, so refreshes are serialized across
 * the browser's tabs, and reads wait while one is in flight. Risk-reducing commands never wait (API-7).
 */
import type { Fetch } from "./client";

/** What the sign-in client gives: its access token, none (signed out), or `unreachable` (a provider outage). */
export type AccessToken = string | null | "unreachable";

/**
 * How a session call ended. `outage`: the session keeps only pause and the kill switch (§3.11 item 6).
 * `refused`: the API's one 401 or a 403, so the token or the call was not accepted. `unreachable`: no
 * answer, or any other status (a deploy, a drain's `busy`, a 5xx), which the caller tries again.
 */
export type SessionState = "open" | "outage" | "signed_out" | "refused" | "unreachable";

/** The part of the Web Locks API the keeper uses (`navigator.locks`). */
export interface Locks {
  request<T>(name: string, options: { mode: "shared" | "exclusive" }, run: () => Promise<T>): Promise<T>;
}

export interface SessionOptions {
  origin: string;
  fetch: Fetch;
  accessToken: () => Promise<AccessToken>;
  /** `navigator.locks` where the browser has it; without it, the tab serializes alone. */
  locks?: Locks;
  /** Told after every refresh, so another tab that just refreshed is not followed by a second one. */
  channel?: { post: (at: number) => void; latest: () => number };
  now?: () => number;
}

export interface SessionKeeper {
  open(): Promise<SessionState>;
  refresh(): Promise<SessionState>;
  signOut(): Promise<SessionState>;
  /** Runs `call` while no refresh is in flight in any tab. */
  guard<T>(call: () => Promise<T>): Promise<T>;
}

/** How long a session call may take before it counts as unanswered, so a hung call never holds the lock. */
const SESSION_CALL_TIMEOUT_MS = 10_000;

/** Refresh one minute before the five-minute access lifetime ends. */
export const REFRESH_EVERY_MS = 4 * 60_000;
export const SESSION_LOCK = "owlhead-api-session";

/** One tab's own reader-writer lock, for a browser without `navigator.locks`. */
function tabLocks(): Locks {
  let shared = 0;
  let exclusive = false;
  const waiting: Array<() => void> = [];
  return {
    async request(_name, { mode }, run) {
      const free = () => (mode === "shared" ? !exclusive : !exclusive && shared === 0);
      while (!free()) await new Promise<void>((resolve) => waiting.push(resolve));
      if (mode === "shared") shared += 1;
      else exclusive = true;
      try {
        return await run();
      } finally {
        if (mode === "shared") shared -= 1;
        else exclusive = false;
        waiting.splice(0).forEach((wake) => wake());
      }
    },
  };
}

export function createSessionKeeper(options: SessionOptions): SessionKeeper {
  const { origin, fetch, accessToken, channel, now = Date.now } = options;
  const locks = options.locks ?? tabLocks();

  /** One browser-only call (§3.11 item 5): the API's cookie, the CSRF header, never a token in a header or URL. */
  async function call(method: "POST" | "DELETE", path: string, body?: object): Promise<"ok" | "refused" | "unreachable"> {
    try {
      const response = await fetch(origin + path, {
        method,
        credentials: "include",
        headers: { "Content-Type": "application/json", "X-Mandate-Request": "1" },
        ...(body ? { body: JSON.stringify(body) } : {}),
        signal: AbortSignal.timeout(SESSION_CALL_TIMEOUT_MS),
      });
      if (response.status === 204) return "ok";
      return response.status === 401 || response.status === 403 ? "refused" : "unreachable";
    } catch {
      return "unreachable";
    }
  }

  async function open(): Promise<SessionState> {
    const token = await accessToken();
    if (token === null) return "signed_out";
    if (token === "unreachable") return "unreachable";
    return locks.request(SESSION_LOCK, { mode: "exclusive" }, async (): Promise<SessionState> => {
      const answer = await call("POST", "/v1/sessions", { access_token: token });
      if (answer === "ok") channel?.post(now());
      return answer === "ok" ? "open" : answer;
    });
  }

  async function refresh(): Promise<SessionState> {
    return locks.request(SESSION_LOCK, { mode: "exclusive" }, async (): Promise<SessionState> => {
      if (channel && now() - channel.latest() < REFRESH_EVERY_MS / 2) return "open";
      const token = await accessToken();
      if (token === null) return "signed_out";
      const answer = await call("POST", "/v1/sessions/refresh", { access_token: token === "unreachable" ? null : token });
      if (answer !== "ok") return answer;
      channel?.post(now());
      return token === "unreachable" ? "outage" : "open";
    });
  }

  async function signOut(): Promise<SessionState> {
    return locks.request(SESSION_LOCK, { mode: "exclusive" }, async (): Promise<SessionState> => {
      await call("DELETE", "/v1/sessions/current");
      return "signed_out";
    });
  }

  const guard = <T>(run: () => Promise<T>) => locks.request(SESSION_LOCK, { mode: "shared" }, run);

  return Object.freeze({ open, refresh, signOut, guard });
}
