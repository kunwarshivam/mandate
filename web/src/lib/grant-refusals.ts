/**
 * Why an approval answer did not approve, in words (C-6: the sentence is shown, the code is
 * recorded). Two places refuse a grant. The API refuses one before recording anything (DEC-2129
 * item 7, workspace API spec §5.2): the answer is `undelivered` with the problem's code. The runtime
 * judges every recorded answer again (mandate spec §6.4, checks 1 to 7) and journals
 * `ApprovalResponded` refused with the first check that failed (journal spec rule 47), which
 * `GET /commands/{event_id}` serves as `refused` (§5.5). Neither ever reads as approved.
 */

/** The runtime's refusals, one per `ApprovalResponded` reason (journal spec §9). */
const REFUSED: Readonly<Record<string, string>> = {
  not_pending: "Refused: this request was no longer waiting when your approval was judged. Nothing was approved.",
  late: "Refused: your approval arrived after the request's deadline. Nothing was approved.",
  not_an_approver: "Refused: you are not an approver of this request now. Nothing was approved.",
  not_delivered: "Refused: this request was never delivered to you. Nothing was approved.",
  content_mismatch: "Refused: your approval was for other content than this request holds. Nothing was approved.",
  step_up_missing: "Refused: your deployment found no confirmation with your approval. Nothing was approved.",
  step_up_stale: "Refused: your confirmation was too old when your approval was judged. Nothing was approved.",
  step_up_reused: "Refused: this confirmation was already used for another answer. Nothing was approved.",
  step_up_method: "Refused: this kind of confirmation is not taken for this request. Nothing was approved.",
  duplicate_approver: "Refused: you have already approved this request, and a second approval from you does not count. Nothing more was approved.",
  not_independent: "Refused: this request needs an approver other than the one who set it up. Nothing was approved.",
};

/** A recorded answer the runtime refused, for `reason`, the refusing step's code. */
export function refusedText(reason: string | null): string {
  return (reason && REFUSED[reason]) || "Refused by your deployment. Nothing was approved.";
}

/** Why an answer was not recorded: the step-up refusals an approve can meet (§3.5, DEC-2104 item 6), and the API's approver check. */
const UNDELIVERED: Readonly<Record<string, string>> = {
  step_up_required: "Not recorded: approving needs a confirmation your deployment did not receive, so nothing was approved.",
  step_up_missing: "Not recorded: your deployment found no confirmation of this code on the host, so nothing was approved. Run the command, then approve again.",
  step_up_mismatch: "Not recorded: the confirmation on the host was for another request, so nothing was approved.",
  step_up_stale: "Not recorded: the code expired before it was used, so nothing was approved. Ask for a new code.",
  step_up_reused: "Not recorded: this code was already used, so nothing was approved. Ask for a new code.",
  step_up_method: "Not recorded: your deployment does not take a host confirmation for this, so nothing was approved.",
  forbidden: "Not recorded: you are not an approver of this request, so nothing was approved.",
  role_forbidden: "Not recorded: your role makes you not an approver of this request, so nothing was approved.",
};

/** An answer the API recorded nothing for, for `reason`, the problem's code. */
export function undeliveredText(reason: string | null | undefined): string {
  return (reason && UNDELIVERED[reason]) || "Not recorded: your deployment answered that nothing was recorded, so nothing was approved.";
}

/** The API's refusals that no answer from this person can get past: the screen offers nothing more to send. */
export const FINAL_UNDELIVERED: ReadonlySet<string> = new Set(["forbidden", "role_forbidden"]);
