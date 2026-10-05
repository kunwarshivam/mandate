"use client";

import Link from "next/link";
import { type FormEvent, useState } from "react";
import { CircleNotch, EnvelopeSimple, Fingerprint, GoogleLogo } from "@phosphor-icons/react";
import { CALLBACK_PATH } from "@/lib/auth-routes";
import { EMAIL_SENT, SIGN_IN_FAILED, SIGN_IN_PASSKEY_COPY, UNREACHABLE, isNetworkFailure, passkeyProblem, webAuthnSupported } from "@/lib/auth-errors";
import { type BrowserClient, createClient } from "@/lib/supabase/client";
import { LOGON_FIELD, LOGON_LINK, LOGON_NOTICE, LOGON_PRIMARY, LOGON_SECONDARY } from "./logon-styles";
import { LogonHeading } from "./logon-heading";

export type LoginAuth = Pick<BrowserClient["auth"], "signInWithOAuth" | "signInWithPasskey" | "signInWithOtp">;

type Pending = "google" | "passkey" | "email" | null;

export interface LoginPanelProps {
  /** Where to go once signed in: already a safe, same-origin path (`safeNext`). */
  next: string;
  /** The callback came back with `?error=1`. */
  failed: boolean;
  enabled: boolean;
  emailEnabled: boolean;
  /** Tests pass a stub; the page uses the browser client. */
  auth?: LoginAuth;
  navigate?: (href: string) => void;
}

export function callbackUrl(origin: string, next: string): string {
  return `${origin}${CALLBACK_PATH}?next=${encodeURIComponent(next)}`;
}

function Busy() {
  return <CircleNotch className="size-5 shrink-0 motion-safe:animate-spin" aria-hidden />;
}

/**
 * The sign-in page (brief O1, DEC-211): Google first, which also creates the account, then a passkey
 * for every later visit, and an email link only while its flag is on. Every failure reads the same
 * whether or not an account exists; the email form always answers with one confirmation.
 */
export function LoginPanel({ next, failed, enabled, emailEnabled, auth, navigate = (href) => window.location.assign(href) }: LoginPanelProps) {
  const [pending, setPending] = useState<Pending>(null);
  const [message, setMessage] = useState<string | null>(failed ? SIGN_IN_FAILED : null);
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const client = () => auth ?? createClient().auth;

  if (!enabled) {
    return (
      <section aria-labelledby="login-title" className="grid gap-5" data-slot="login">
        <LogonHeading id="login-title">
          Sign in
        </LogonHeading>
        <p className={LOGON_NOTICE}>Sign-in is off in this build. It runs on fixture data and opens without an account.</p>
        <Link href="/" className={LOGON_PRIMARY}>
          Open Owlhead
        </Link>
      </section>
    );
  }

  async function google() {
    setPending("google");
    setMessage(null);
    const { error } = await client().signInWithOAuth({ provider: "google", options: { redirectTo: callbackUrl(window.location.origin, next) } });
    if (error) {
      setPending(null);
      setMessage(isNetworkFailure(error) ? UNREACHABLE : SIGN_IN_FAILED);
    }
  }

  async function passkey() {
    setMessage(null);
    if (!webAuthnSupported()) {
      setMessage(SIGN_IN_PASSKEY_COPY.unsupported);
      return;
    }
    setPending("passkey");
    const { data, error } = await client().signInWithPasskey();
    if (error || !data?.session) {
      setPending(null);
      setMessage(isNetworkFailure(error) ? UNREACHABLE : SIGN_IN_PASSKEY_COPY[passkeyProblem(error)]);
      return;
    }
    navigate(next);
  }

  async function sendLink(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const address = email.trim();
    if (address === "") return;
    setPending("email");
    setMessage(null);
    const { error } = await client().signInWithOtp({
      email: address,
      options: { emailRedirectTo: callbackUrl(window.location.origin, next), shouldCreateUser: false },
    });
    setPending(null);
    if (isNetworkFailure(error)) {
      setMessage(UNREACHABLE);
      return;
    }
    setSent(true);
  }

  const busy = pending !== null;
  const status = pending === "google" ? "Opening Google…" : pending === "passkey" ? "Waiting for your passkey…" : pending === "email" ? "Sending…" : "";

  return (
    <section aria-labelledby="login-title" aria-busy={busy} className="grid gap-5" data-slot="login">
      <div className="grid gap-2">
        <LogonHeading id="login-title">
          Sign in
        </LogonHeading>
        <p className="text-pretty">New to Owlhead? Continue with Google to create your account. After that, a passkey signs you in.</p>
      </div>

      {message ? (
        <p role="alert" className={LOGON_NOTICE} data-slot="login-message">
          {message}
        </p>
      ) : null}

      <div className="grid gap-2">
        <button type="button" onClick={google} disabled={busy} className={LOGON_PRIMARY}>
          {pending === "google" ? <Busy /> : <GoogleLogo className="size-5 shrink-0" weight="bold" aria-hidden />}
          Continue with Google
        </button>
        <button type="button" onClick={passkey} disabled={busy} className={LOGON_SECONDARY}>
          {pending === "passkey" ? <Busy /> : <Fingerprint className="size-5 shrink-0" aria-hidden />}
          Sign in with a passkey
        </button>
      </div>

      {emailEnabled ? (
        <div className="grid gap-3 border-t border-t-foreground/45 pt-4 shadow-[inset_0_1px_0_var(--card)]" data-slot="login-email">
          {sent ? (
            <>
              <p role="status" className={LOGON_NOTICE}>
                {EMAIL_SENT}
              </p>
              <button
                type="button"
                onClick={() => {
                  setSent(false);
                  setEmail("");
                }}
                className={LOGON_LINK}
              >
                Use a different address
              </button>
            </>
          ) : (
            <form onSubmit={sendLink} className="grid gap-2" noValidate>
              <label htmlFor="login-email">
                Or get a sign-in link by email
              </label>
              <input
                id="login-email"
                type="email"
                inputMode="email"
                autoComplete="email"
                spellCheck={false}
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="you@example.com"
                className={LOGON_FIELD}
              />
              <button type="submit" disabled={busy || email.trim() === ""} className={LOGON_SECONDARY}>
                {pending === "email" ? <Busy /> : <EnvelopeSimple className="size-5 shrink-0" aria-hidden />}
                Email me a link
              </button>
            </form>
          )}
        </div>
      ) : null}

      <p role="status" aria-live="polite" className="sr-only">
        {status}
      </p>
    </section>
  );
}
