"use client";

import { cliConfirmCode, cliConfirmEvidence } from "@/api/step-up";
import type { StepUpEvidence } from "@/api/types";
import { DECISION_KEY, KEY } from "@/components/kumo/key";
import { Panel } from "@/components/screens/common";
import { clock } from "@/lib/format";
import type { StepUpRequest } from "@/lib/mock-runtime";

/** Why no challenge was issued, in words; the code goes to the record, never the sentence alone (C-6). */
function notIssued(reason: string): string {
  if (reason === "live_unavailable") return "Approving from here is for paper only, and this workspace is live, so nothing was approved.";
  if (reason === "network") return "Your deployment did not answer, so no code was issued and nothing was approved. Try again.";
  return "Your deployment did not issue a confirmation code, so nothing was approved. Try again, or approve from the CLI.";
}

/**
 * Approving with `cli_confirm` (DEC-2104): the code and the one command to run on the host, then the
 * grant with evidence naming the challenge. Nothing is approved until the owner sends it, and the
 * server still refuses it until the host has the owner's confirmation. Laid out for a 390 px phone:
 * the command wraps rather than scrolling sideways.
 */
export function ApproveStepUp({ request, onApprove, onCancel }: { request: StepUpRequest | "asking"; onApprove: (evidence: StepUpEvidence) => void; onCancel: () => void }) {
  if (request === "asking") {
    return (
      <p role="status" className="text-sm">
        Asking your deployment for a confirmation code.
      </p>
    );
  }
  if (!request.ok) {
    return (
      <div className="grid gap-3" data-slot="step-up-unavailable">
        <p role="status" className="text-sm">
          {notIssued(request.reason)}
        </p>
        <button type="button" className={KEY} onClick={onCancel}>
          Back
        </button>
      </div>
    );
  }
  const { challenge, line } = request;
  return (
    <Panel well>
      <section className="grid min-w-0 gap-4" aria-labelledby="step-up-title" data-slot="step-up">
        <h2 id="step-up-title" className="text-h3">
          Confirm on your computer
        </h2>
        <p className="text-sm text-muted-foreground">
          Run this on the computer that hosts your deployment. It shows the request from the journal; check it matches this one. Paper only. Replace the principal with yours.
        </p>
        <p className="font-mono text-hero tracking-widest" aria-label="Confirmation code">
          {cliConfirmCode(challenge.webauthn_challenge)}
        </p>
        <pre className="min-w-0 whitespace-pre-wrap break-all rounded-md bg-background p-3 font-mono text-xs" aria-label="Command to run">
          {line}
        </pre>
        <p className="text-sm text-muted-foreground">The code works once, until {clock(challenge.expires_at)} UTC.</p>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <button type="button" className={DECISION_KEY} onClick={() => onApprove(cliConfirmEvidence(challenge))}>
            I confirmed it: approve
          </button>
          <button type="button" className={KEY} onClick={onCancel}>
            Cancel
          </button>
        </div>
      </section>
    </Panel>
  );
}
