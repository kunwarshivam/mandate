/**
 * The browser's session with the workspace API (workspace API spec §3.11 items 5 and 6; DEC-975's next
 * slice). The app trades the sign-in client's access token for the API's own session cookie with
 * `POST /v1/sessions`, refreshes it with `POST /v1/sessions/refresh` before its five-minute access
 * lifetime ends, and ends it with `DELETE /v1/sessions/current`. A refresh rotates the cookie's
 * secret and a rotated secret presented again ends the session, so refreshes are serialized across
 * the browser's tabs, and reads wait while one is in flight. Risk-reducing commands never wait (API-7).
 */
import type { Fetch } from "./client";

/** What the sign-in client gives: its access token, none (signed out), or `unreachable` (a provider outage). */
export type AccessToken = string | null | "unreachable";

/** How a session call ended. `outage`: the session keeps only pause and the kill switch (§3.11 item 6). */
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

/** Refresh one minute before the five-minute access lifetime ends. */
export const REFRESH_EVERY_MS = 4 * 60_000;
export const SESSION_LOCK = "owlhead-api-session";

export function createSessionKeeper(options: SessionOptions): SessionKeeper {
  void options;
  throw new Error("Unimplemented: E11-9");
}
