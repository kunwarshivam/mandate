"use client";

import { useEffect, useState } from "react";
import { type ShownChallenge, challengeLifetimeMs } from "@/api/step-up";
import { DECISION_KEY, KEY } from "@/components/kumo/key";
import { Panel } from "@/components/screens/common";
import { cn } from "@/lib/utils";

/**
 * The `cli_confirm` step-up as the owner sees it (DEC-2104): the code made from the challenge, the
 * command that confirms it on the deployment's host, and when it expires. The browser cannot confirm
 * it; pressing `submit` only sends the grant, which the server refuses unless the host has confirmed
 * this code. Nothing here is stored, and nothing is copied for the owner (no clipboard, as everywhere
 * in the app). Mount one per challenge (`key`): the code's life is counted from when it was shown, so
 * this device's clock cannot make it look current, and the server judges the window itself.
 */
export function HostConfirm({
  shown,
  command,
  submitLabel,
  onSubmit,
  onCancel,
}: {
  shown: ShownChallenge;
  command: string;
  submitLabel: string;
  onSubmit: () => void;
  onCancel: () => void;
}) {
  const [current, setCurrent] = useState(true);
  useEffect(() => {
    const id = window.setTimeout(() => setCurrent(false), challengeLifetimeMs(shown.challenge));
    return () => window.clearTimeout(id);
  }, [shown.challenge]);
  return (
    <Panel well>
      <section className="grid gap-4" aria-labelledby="host-confirm-title" data-slot="host-confirm">
        <h2 id="host-confirm-title" className="text-h3">
          Confirm on the host
        </h2>
        <p className="text-sm text-muted-foreground">
          Run this on your deployment&rsquo;s host. It shows this request as the journal holds it; confirm it there, then come back. Paper account; simulated funds.
        </p>
        <p className="font-mono text-h1 tabular-nums" data-slot="host-confirm-code" aria-label={`Code ${shown.code.split("").join(" ")}`}>
          {shown.code}
        </p>
        <pre className="overflow-x-auto rounded-xl bg-background px-4 py-3 text-xs whitespace-pre-wrap break-all" data-slot="host-confirm-command">
          {command}
        </pre>
        <p role="status" aria-live="polite" className="text-sm text-muted-foreground">
          {current ? `The code expires ${Math.round(challengeLifetimeMs(shown.challenge) / 60_000)} minutes after it was shown.` : "This code has expired. Cancel and ask for a new one; nothing was approved."}
        </p>
        <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
          <button type="button" className={cn("min-w-28", KEY)} onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className={DECISION_KEY} onClick={onSubmit} disabled={!current}>
            {submitLabel}
          </button>
        </div>
      </section>
    </Panel>
  );
}
