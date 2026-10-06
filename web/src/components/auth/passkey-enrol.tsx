"use client";

import Link from "next/link";
import { useState } from "react";
import { Key } from "pixelarticons/react/Key.js";
import { Loader } from "pixelarticons/react/Loader.js";
import { ADD_PASSKEY_COPY, passkeyProblem, webAuthnSupported } from "@/lib/auth-errors";
import { LOGIN_PATH } from "@/lib/auth-routes";
import { type BrowserClient, createClient } from "@/lib/supabase/client";
import { LOGON_LINK, LOGON_NOTICE, LOGON_PRIMARY, LOGON_SECONDARY } from "./logon-styles";
import { LogonHeading } from "./logon-heading";

export type EnrolAuth = Pick<BrowserClient["auth"], "registerPasskey">;

/**
 * The one-time step after a first sign-in with Google or an email link (DEC-211): add a passkey, or
 * skip it. Adding needs a session, which Supabase checks; without one the page says so and links
 * back to sign in.
 */
export function PasskeyEnrol({
  next,
  enabled,
  auth,
  navigate = (href) => window.location.assign(href),
}: {
  next: string;
  enabled: boolean;
  auth?: EnrolAuth;
  navigate?: (href: string) => void;
}) {
  const [pending, setPending] = useState(false);
  const [problem, setProblem] = useState<keyof typeof ADD_PASSKEY_COPY | null>(null);

  async function add() {
    setProblem(null);
    if (!webAuthnSupported()) {
      setProblem("unsupported");
      return;
    }
    setPending(true);
    const { error } = await (auth ?? createClient().auth).registerPasskey();
    setPending(false);
    if (error) {
      setProblem(passkeyProblem(error));
      return;
    }
    navigate(next);
  }

  return (
    <section aria-labelledby="enrol-title" aria-busy={pending} className="grid gap-5" data-slot="passkey-enrol">
      <div className="grid gap-2">
        <LogonHeading id="enrol-title">
          Add a passkey
        </LogonHeading>
        <p className="text-pretty">Next time, sign in with your fingerprint, face or device PIN instead of Google.</p>
      </div>

      {problem ? (
        <p role="alert" className={LOGON_NOTICE}>
          {ADD_PASSKEY_COPY[problem]}
          {problem === "signed-out" ? (
            <>
              {" "}
              <Link href={LOGIN_PATH} className={LOGON_LINK}>
                Sign in
              </Link>
            </>
          ) : null}
        </p>
      ) : null}

      <div className="grid gap-2">
        <button type="button" onClick={add} disabled={pending || !enabled} className={LOGON_PRIMARY}>
          {pending ? <Loader className="size-6 shrink-0 motion-safe:animate-spin" aria-hidden /> : <Key className="size-6 shrink-0" aria-hidden />}
          Add a passkey
        </button>
        <Link href={next} className={LOGON_SECONDARY}>
          Not now
        </Link>
      </div>

      <p role="status" aria-live="polite" className="sr-only">
        {pending ? "Waiting for your device…" : ""}
      </p>
    </section>
  );
}
