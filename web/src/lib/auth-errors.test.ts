import { AuthApiError, AuthRetryableFetchError, AuthSessionMissingError, AuthUnknownError } from "@supabase/supabase-js";
import { describe, expect, it } from "vitest";
import { ADD_PASSKEY_COPY, EMAIL_SENT, SIGN_IN_FAILED, SIGN_IN_PASSKEY_COPY, UNREACHABLE, isNetworkFailure, passkeyProblem } from "./auth-errors";

const browser = (name: string, code: string) => Object.assign(new Error("x"), { name, code });

describe("passkeyProblem", () => {
  it.each([
    [new AuthApiError("disabled", 422, "passkey_disabled"), "disabled"],
    [new AuthUnknownError("Browser does not support WebAuthn", null), "unsupported"],
    [browser("NotAllowedError", "ERROR_PASSTHROUGH_SEE_CAUSE_PROPERTY"), "cancelled"],
    [browser("AbortError", "ERROR_CEREMONY_ABORTED"), "cancelled"],
    [browser("InvalidStateError", "ERROR_AUTHENTICATOR_PREVIOUSLY_REGISTERED"), "exists"],
    [new AuthApiError("exists", 422, "webauthn_credential_exists"), "exists"],
    [new AuthSessionMissingError(), "signed-out"],
    [new AuthApiError("not found", 404, "webauthn_credential_not_found"), "failed"],
    [new AuthApiError("expired", 400, "webauthn_challenge_expired"), "failed"],
    [new AuthApiError("banned", 403, "user_banned"), "failed"],
    [new AuthApiError("unconfirmed", 400, "email_not_confirmed"), "failed"],
    [null, "failed"],
    ["text", "failed"],
  ] as const)("reads %s as %s", (error, problem) => {
    expect(passkeyProblem(error)).toBe(problem);
  });

  it("tells a network failure apart, and nothing else", () => {
    expect(isNetworkFailure(new AuthRetryableFetchError("Failed to fetch", 0))).toBe(true);
    expect(isNetworkFailure(new AuthApiError("rate", 429, "over_email_send_rate_limit"))).toBe(false);
    expect(isNetworkFailure(null)).toBe(false);
  });
});

describe("the sign-in copy never says whether an email or an account exists (brief O1)", () => {
  const all = [...Object.values(SIGN_IN_PASSKEY_COPY), ...Object.values(ADD_PASSKEY_COPY).filter((t) => t !== ADD_PASSKEY_COPY.exists), SIGN_IN_FAILED, EMAIL_SENT, UNREACHABLE];

  it.each(all)("%s", (text) => {
    expect(text).not.toMatch(/not found|no account|exist|unknown (email|user|address)|not registered|no user|sign(ed)? up|!/i);
  });

  it("says passkeys are off in the founder's words, pointing to Google", () => {
    expect(SIGN_IN_PASSKEY_COPY.disabled).toBe("Passkeys aren’t turned on yet; continue with Google.");
  });
});
