/**
 * Why a passkey step-up sent nothing, or why the deployment did not accept one, in words (rule 6:
 * a server's code stays out of the sentence). Shared by every screen that asks for a passkey, so the
 * same refusal reads the same everywhere; `null` for a code that is not a step-up's.
 */
export function stepUpWords(reason: string | undefined): string | null {
  switch (reason) {
    case "step_up_required":
      return "Not recorded: your deployment asked for a passkey check that was not sent.";
    case "step_up_missing":
    case "step_up_stale":
    case "step_up_method":
      return "Not recorded: your deployment did not accept the passkey check.";
    case "passkey_unavailable":
      return "Not sent: this browser can't use a passkey.";
    case "no_passkey":
      return "Not sent: you have no passkey on your deployment yet. Add one, then try again.";
    case "passkey_cooling_off":
      return "Not sent: your passkey is new, and confirms only once its first 24 hours have passed.";
    case "passkey_canceled":
      return "Not sent: the passkey check was canceled or timed out.";
    case "passkey_failed":
      return "Not sent: your passkey's answer could not be read.";
    default:
      return null;
  }
}
