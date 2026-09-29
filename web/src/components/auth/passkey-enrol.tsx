"use client";

import Link from "next/link";
import { useState } from "react";
import { CircleNotch, Fingerprint } from "@phosphor-icons/react";
import { ADD_PASSKEY_COPY, passkeyProblem, webAuthnSupported } from "@/lib/auth-errors";
import { LOGIN_PATH } from "@/lib/auth-routes";
import { type BrowserClient, createClient } from "@/lib/supabase/client";
import { NOTICE, OUTLINE_PILL, PRIMARY_PILL } from "./buttons";

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
    <section aria-labelledby="enrol-title" aria-busy={pending} className="mx-auto grid w-full max-w-sm gap-8 pt-6 sm:pt-16" data-slot="passkey-enrol">
      <div className="grid gap-3">
        <Fingerprint className="size-10 text-lapis" aria-hidden />
        <h1 id="enrol-title" className="text-h1">
          Add a passkey
        </h1>
        <p className="text-muted-foreground">Next time, sign in with your fingerprint, face or device PIN instead of Google.</p>
      </div>

      {problem ? (
        <p role="alert" className={NOTICE}>
          {ADD_PASSKEY_COPY[problem]}
          {problem === "signed-out" ? (
            <>
              {" "}
              <Link href={LOGIN_PATH} className="font-semibold underline decoration-lapis/30 underline-offset-4 hover:decoration-current">
                Sign in
              </Link>
            </>
          ) : null}
        </p>
      ) : null}

      <div className="grid gap-3">
        <button type="button" onClick={add} disabled={pending || !enabled} className={PRIMARY_PILL}>
          {pending ? <CircleNotch className="size-5 shrink-0 motion-safe:animate-spin" aria-hidden /> : null}
          Add a passkey
        </button>
        <Link href={next} className={OUTLINE_PILL}>
          Not now
        </Link>
      </div>

      <p role="status" aria-live="polite" className="sr-only">
        {pending ? "Waiting for your device…" : ""}
      </p>
    </section>
  );
}
