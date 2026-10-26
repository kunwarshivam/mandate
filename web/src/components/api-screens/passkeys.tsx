"use client";

import { useCallback, useEffect, useState } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import type { Enrolled, ListedPasskey } from "@/api/enrol";
import { KEY } from "@/components/kumo/key";
import { Section } from "@/components/screens/common";
import { datedClock } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";

/**
 * Your passkeys on this deployment (§3.6, DEC-2108): the ones the control stream holds, and adding
 * one. These are the passkeys a confirmation, a deployment or an approval's step-up asks for; they
 * are not the sign-in passkeys. A first passkey can step up once its 24 hours of cool-off have
 * passed (DEC-2108 item 5). Nothing reads as added before the deployment says so, and a lost answer
 * reads the list again rather than add twice.
 */

/** A served nine-digit timestamp against this page's clock, as instants. */
const after = (served: string, now: string) => Date.parse(`${served.slice(0, 23)}Z`) > Date.parse(now);

type Outcome = { kind: "added"; enrolled: Enrolled } | { kind: "refused"; reason: string } | { kind: "unknown" };

/** Why nothing was added, in words; a server's code stays out of the sentence. */
function refusedWords(reason: string): string {
  switch (reason) {
    case "passkey_unavailable":
      return "This browser can't make a passkey.";
    case "passkey_canceled":
      return "The passkey check was canceled or timed out.";
    case "passkey_failed":
      return "Your authenticator's answer could not be read.";
    case "no_passkey":
    case "passkey_cooling_off":
      return "Adding another passkey needs one you already hold, past its first 24 hours.";
    case "step_up_stale":
      return "A first passkey must be added within five minutes of signing in. Sign in again, then add it.";
    case "step_up_missing":
    case "step_up_mismatch":
    case "step_up_reused":
      return "Your deployment did not accept the passkey check.";
    case "rate_limited":
      return "Too many passkey requests at once. Wait a minute, then try again.";
    default:
      return "Your deployment refused it.";
  }
}

function PasskeyList({ passkeys, now }: { passkeys: readonly ListedPasskey[]; now: string }) {
  if (passkeys.length === 0) return <p className="text-muted-foreground">You have no passkey on this deployment yet.</p>;
  return (
    <ul className="grid" aria-label="Your passkeys on this deployment">
      {passkeys.map((p) => (
        <li key={p.passkey_id} className="flex flex-wrap justify-between gap-x-3 gap-y-0.5 border-b border-border/70 py-2.5 text-sm last:border-b-0" data-passkey={p.passkey_id}>
          <span>
            Passkey <span className="font-mono">…{p.passkey_id.slice(-6)}</span>, added {datedClock(p.enrolled_at, now, "UTC")} UTC
          </span>
          <span className="text-muted-foreground">
            {p.cool_off_ends !== null && after(p.cool_off_ends, now) ? `Can confirm from ${datedClock(p.cool_off_ends, now, "UTC")} UTC` : "Can confirm"}
          </span>
        </li>
      ))}
    </ul>
  );
}

export function ApiPasskeys() {
  const { passkeys, now } = useRuntime();
  const [listed, setListed] = useState<{ ok: true; passkeys: ListedPasskey[] } | { ok: false; reason: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const load = useCallback(async () => {
    if (!passkeys) return;
    setListed(await passkeys.list());
  }, [passkeys]);
  useEffect(() => {
    if (!passkeys) return;
    let live = true;
    void passkeys.list().then((answer) => live && setListed(answer));
    return () => {
      live = false;
    };
  }, [passkeys]);
  if (!passkeys) return null;
  const add = async () => {
    setBusy(true);
    setOutcome(null);
    const answer = await passkeys.enrol();
    setOutcome(answer.ok ? { kind: "added", enrolled: answer.enrolled } : answer.effect === "unknown" ? { kind: "unknown" } : { kind: "refused", reason: answer.reason });
    setBusy(false);
    await load();
  };
  return (
    <Section title="Passkeys for confirming">
      <p className="text-sm text-muted-foreground">
        Your deployment asks for one of these when you confirm a mandate, deploy an agent or approve a request. They are not your sign-in passkeys.
      </p>
      {listed === null ? <p className="text-muted-foreground">Reading your passkeys.</p> : listed.ok ? <PasskeyList passkeys={listed.passkeys} now={now} /> : <p className="text-muted-foreground">Your passkeys are not reported.</p>}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <Button type="button" variant="secondary" size="lg" className={cn(KEY, "w-full sm:w-auto")} disabled={busy} onClick={() => void add()}>
          Add a passkey
        </Button>
        <p className="text-sm text-muted-foreground">Your browser asks your authenticator to make one.</p>
      </div>
      <div role="status" aria-live="polite">
        {busy ? <p className="text-sm">Waiting for your authenticator. Nothing is added until your deployment records it.</p> : null}
        {outcome?.kind === "added" ? (
          <p className="text-sm" data-slot="passkey-added">
            Recorded in the journal: passkey …{outcome.enrolled.passkey_id.slice(-6)} added.
            {outcome.enrolled.cool_off_ends ? ` It can confirm from ${datedClock(outcome.enrolled.cool_off_ends, now, "UTC")} UTC, after its first 24 hours.` : " It can confirm now."}
          </p>
        ) : null}
        {outcome?.kind === "refused" ? <p className="text-sm" data-reason={outcome.reason}>Not added: {refusedWords(outcome.reason)}</p> : null}
        {outcome?.kind === "unknown" ? <p className="text-sm">Whether the passkey was added is not known yet; the list above is read again. Check it before adding another.</p> : null}
      </div>
    </Section>
  );
}
