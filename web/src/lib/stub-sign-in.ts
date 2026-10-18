/**
 * The web app's sign-in token in a dev or CI cell (DEC-2406): E21-52's stub issuer stands in for the
 * sign-in client, as DEC-2100 item 11 says it does. The stub sends no CORS headers (DEC-2100 item 7),
 * so the page cannot ask it; `next dev`'s own server asks it on loopback, for the one demo subject the
 * server's configuration names, and hands the browser that token, which it trades at
 * `POST /v1/sessions` like Supabase's (workspace API spec §3.11 item 5). It is on only under
 * `next dev`: a production build compiles no route for it (`route.dev.ts`) and its middleware answers
 * 404 on `/dev/` first.
 *
 * `OWLHEAD_STUB_ISSUER` is the stub's issuer exactly as it printed it, `http://127.0.0.1:<port>` or
 * `http://[::1]:<port>` (DEC-2100 items 5 and 6), and `OWLHEAD_STUB_SUBJECT` the subject an operator
 * linked for it (`mandate identity link --issuer`). Neither is a secret, and neither reaches the bundle.
 */
import type { AccessToken } from "@/api/session";

export const STUB_ISSUER_ENV = "OWLHEAD_STUB_ISSUER";
export const STUB_SUBJECT_ENV = "OWLHEAD_STUB_SUBJECT";
/** The app's own route the browser asks for the token; `route.dev.ts`, so a production build has none. */
export const STUB_TOKEN_PATH = "/dev/stub-token";
/** The CSRF mark the route requires, the API's own header (spec §3.3), so a cross-site form cannot send it. */
export const STUB_TOKEN_HEADER = "X-Mandate-Request";

export interface StubSignIn {
  issuer: string;
  subject: string;
}

/** An IP literal on loopback and a port 1 to 65535 with no leading zero, and nothing else (DEC-2100 item 5). */
const ISSUER = /^http:\/\/(127\.0\.0\.1|\[::1\]):([1-9][0-9]{0,4})$/;
/** DEC-992 item 2's subject: 1 to 1024 bytes and no control character. */
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;
const MAX_SUBJECT_BYTES = 1024;
/** How long the stub may take, so a hung stub never holds the sign-in. */
const MINT_TIMEOUT_MS = 10_000;

/** The configured stub sign-in, or `null`: outside `next dev`, unset, or not parsed, it is off. */
export function readStubSignIn(env: Readonly<Record<string, string | undefined>>, nodeEnv: string | undefined): StubSignIn | null {
  if (nodeEnv !== "development") return null;
  const issuer = env[STUB_ISSUER_ENV] ?? "";
  const subject = env[STUB_SUBJECT_ENV] ?? "";
  const port = ISSUER.exec(issuer)?.[2];
  if (!port || Number(port) > 65_535) return null;
  if (subject === "" || CONTROL.test(subject) || new TextEncoder().encode(subject).length > MAX_SUBJECT_BYTES) return null;
  return { issuer, subject };
}

export type Minted = { ok: true; accessToken: string } | { ok: false };

/** `POST <issuer>/token` with `{"subject"}` (DEC-2100 item 7); only a 200 with a string `access_token` is a token. */
export async function mintStubToken(config: StubSignIn, fetch: typeof globalThis.fetch): Promise<Minted> {
  try {
    const response = await fetch(`${config.issuer}/token`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ subject: config.subject }),
      redirect: "error",
      cache: "no-store",
      signal: AbortSignal.timeout(MINT_TIMEOUT_MS),
    });
    if (response.status !== 200) return { ok: false };
    const body: unknown = await response.json();
    const token = typeof body === "object" && body !== null ? (body as Record<string, unknown>).access_token : undefined;
    return typeof token === "string" && token !== "" ? { ok: true, accessToken: token } : { ok: false };
  } catch {
    return { ok: false };
  }
}

/** Whether the browser asked from this app's own origin, with the CSRF mark: `Origin`'s host is the `Host` it sent. */
export function sameOriginAsk(headers: Headers): boolean {
  if (headers.get(STUB_TOKEN_HEADER) !== "1") return false;
  const origin = headers.get("origin");
  const host = headers.get("host");
  if (!origin || !host) return false;
  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

/**
 * The browser's side: the token from the app's own route. No route (404) or a refused ask (403) is
 * signed out; a stub that did not answer (502), or no answer at all, is `unreachable`, which the API
 * session reads as the provider's outage (§3.11 item 6).
 */
export async function tokenFromStub(fetch: (input: string, init: RequestInit) => Promise<Response>): Promise<AccessToken> {
  try {
    const response = await fetch(STUB_TOKEN_PATH, { method: "POST", credentials: "same-origin", cache: "no-store", headers: { [STUB_TOKEN_HEADER]: "1" } });
    if (response.status === 404 || response.status === 403) return null;
    if (response.status !== 200) return "unreachable";
    const body: unknown = await response.json();
    const token = typeof body === "object" && body !== null ? (body as Record<string, unknown>).access_token : undefined;
    return typeof token === "string" && token !== "" ? token : "unreachable";
  } catch {
    return "unreachable";
  }
}
