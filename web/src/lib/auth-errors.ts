/**
 * What went wrong with a passkey, read from what Supabase returns. The words shown for each never
 * say whether an account or an email exists (brief O1).
 */
export type PasskeyProblem = "cancelled" | "unsupported" | "disabled" | "exists" | "signed-out" | "failed";

interface ErrorShape {
  name?: unknown;
  code?: unknown;
  message?: unknown;
}

function shape(error: unknown): ErrorShape {
  return typeof error === "object" && error !== null ? (error as ErrorShape) : {};
}

/** Whether this browser has the WebAuthn API at all; Supabase checks the same before a ceremony. */
export function webAuthnSupported(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.PublicKeyCredential === "function" &&
    typeof navigator.credentials?.get === "function" &&
    typeof navigator.credentials?.create === "function"
  );
}

export function passkeyProblem(error: unknown): PasskeyProblem {
  const { name, code, message } = shape(error);
  if (code === "passkey_disabled") return "disabled";
  if (message === "Browser does not support WebAuthn") return "unsupported";
  if (code === "ERROR_CEREMONY_ABORTED" || name === "NotAllowedError" || name === "AbortError") return "cancelled";
  if (code === "webauthn_credential_exists" || code === "ERROR_AUTHENTICATOR_PREVIOUSLY_REGISTERED") return "exists";
  if (name === "AuthSessionMissingError" || code === "session_not_found" || code === "no_authorization") return "signed-out";
  return "failed";
}

/** On the sign-in page, where Google is always the other way in. */
export const SIGN_IN_PASSKEY_COPY: Record<PasskeyProblem, string> = {
  cancelled: "The passkey request was cancelled or timed out. Nothing changed; try again, or continue with Google.",
  unsupported: "This browser can’t use passkeys. Continue with Google instead.",
  disabled: "Passkeys aren’t turned on yet; continue with Google.",
  exists: "That passkey didn’t sign you in. Try again, or continue with Google.",
  "signed-out": "That passkey didn’t sign you in. Try again, or continue with Google.",
  failed: "That passkey didn’t sign you in. Try again, or continue with Google.",
};

/** Adding a passkey, after sign-in or from the profile. */
export const ADD_PASSKEY_COPY: Record<PasskeyProblem, string> = {
  cancelled: "No passkey was added. The request was cancelled or timed out.",
  unsupported: "This browser can’t create a passkey.",
  disabled: "Passkeys aren’t turned on yet.",
  exists: "This device’s passkey is already on your account.",
  "signed-out": "Your sign-in has ended. Sign in again to add a passkey.",
  failed: "The passkey wasn’t added. Nothing changed; try again.",
};

/** The one message for any sign-in that did not finish, whatever the cause. */
export const SIGN_IN_FAILED = "That sign-in didn’t finish. Nothing changed; try again.";

/** Shown after any email request, sent or not, so the page never tells whether an address has an account. */
export const EMAIL_SENT = "If that address can sign in, we’ve sent a link. Open it in this browser to finish.";

export const UNREACHABLE = "The sign-in service didn’t answer. Nothing changed; try again in a moment.";

/** A network failure says nothing about an account, so it is the one email error shown as itself. */
export function isNetworkFailure(error: unknown): boolean {
  const { name, code } = shape(error);
  return name === "AuthRetryableFetchError" || code === "request_timeout";
}
