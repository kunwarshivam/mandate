import { AuthApiError, AuthRetryableFetchError, AuthUnknownError } from "@supabase/supabase-js";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EMAIL_SENT, SIGN_IN_FAILED } from "@/lib/auth-errors";
import { type LoginAuth, LoginPanel, type LoginPanelProps, callbackUrl } from "./login-panel";

function stubAuth(overrides: Partial<Record<keyof LoginAuth, ReturnType<typeof vi.fn>>> = {}) {
  return {
    signInWithOAuth: vi.fn().mockResolvedValue({ data: { provider: "google", url: "https://x.supabase.co/auth/v1/authorize" }, error: null }),
    signInWithPasskey: vi.fn().mockResolvedValue({ data: { session: { access_token: "t" }, user: { id: "u" } }, error: null }),
    signInWithOtp: vi.fn().mockResolvedValue({ data: { user: null, session: null }, error: null }),
    ...overrides,
  };
}

function renderLogin(props: Partial<Omit<LoginPanelProps, "auth">> & { auth?: ReturnType<typeof stubAuth> } = {}) {
  const auth = props.auth ?? stubAuth();
  const navigate = vi.fn();
  render(<LoginPanel next="/agents" failed={false} enabled emailEnabled={false} navigate={navigate} {...props} auth={auth as unknown as LoginAuth} />);
  return { auth, navigate };
}

/** A browser error as Supabase's WebAuthn wrapper returns it: the browser's name, Supabase's code. */
function webAuthnError(name: string, code: string) {
  return Object.assign(new Error("The operation either timed out or was not allowed."), { name, code });
}

function withWebAuthn() {
  Object.defineProperty(window, "PublicKeyCredential", { configurable: true, writable: true, value: function PublicKeyCredential() {} });
  Object.defineProperty(navigator, "credentials", { configurable: true, value: { get: vi.fn(), create: vi.fn() } });
}

function withoutWebAuthn() {
  Reflect.deleteProperty(window, "PublicKeyCredential");
  Reflect.deleteProperty(navigator, "credentials");
}

const click = async (name: string | RegExp) => {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name }));
  });
};

/** Words that would tell a visitor whether an address or an account exists. */
const REVEALING = /not found|no account|doesn[’']t exist|does not exist|unknown (email|user|address)|not registered|isn[’']t registered|already (registered|exists)|no user|invalid (email|login)|sign(ed)? up/i;

beforeEach(withWebAuthn);
afterEach(withoutWebAuthn);

describe("the sign-in page", () => {
  it("offers Google and a passkey, and no email field while the flag is off", () => {
    renderLogin();
    expect(screen.getByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue with Google" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Sign in with a passkey" })).toBeEnabled();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByRole("button", { name: "Email me a link" })).toBeNull();
  });

  it("sends Google back to the callback with next, and shows that it is opening Google", async () => {
    const { auth } = renderLogin();
    await click("Continue with Google");
    expect(auth.signInWithOAuth).toHaveBeenCalledWith({
      provider: "google",
      options: { redirectTo: `${window.location.origin}/auth/callback?next=%2Fagents` },
    });
    expect(screen.getByRole("status")).toHaveTextContent("Opening Google…");
    expect(screen.getByRole("button", { name: "Continue with Google" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Sign in with a passkey" })).toBeDisabled();
    expect(screen.getByRole("region", { name: "Sign in" })).toHaveAttribute("aria-busy", "true");
  });

  it("builds the callback address with next encoded", () => {
    expect(callbackUrl("http://localhost:4317", "/approvals?filter=open")).toBe("http://localhost:4317/auth/callback?next=%2Fapprovals%3Ffilter%3Dopen");
  });

  it("goes on to next after a passkey sign-in", async () => {
    let resolve: (value: unknown) => void = () => {};
    const auth = stubAuth({ signInWithPasskey: vi.fn(() => new Promise((r) => (resolve = r))) });
    const { navigate } = renderLogin({ auth });
    await click("Sign in with a passkey");
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for your passkey…");
    expect(screen.getByRole("button", { name: "Sign in with a passkey" })).toBeDisabled();
    await act(async () => resolve({ data: { session: { access_token: "t" }, user: { id: "u" } }, error: null }));
    expect(navigate).toHaveBeenCalledWith("/agents");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it.each([
    ["the browser's NotAllowedError", webAuthnError("NotAllowedError", "ERROR_PASSTHROUGH_SEE_CAUSE_PROPERTY")],
    ["an aborted ceremony", webAuthnError("AbortError", "ERROR_CEREMONY_ABORTED")],
  ])("says a cancelled passkey changed nothing (%s)", async (_what, error) => {
    const auth = stubAuth({ signInWithPasskey: vi.fn().mockResolvedValue({ data: null, error }) });
    const { navigate } = renderLogin({ auth });
    await click("Sign in with a passkey");
    expect(screen.getByRole("alert")).toHaveTextContent("The passkey request was cancelled or timed out. Nothing changed; try again, or continue with Google.");
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Sign in with a passkey" })).toBeEnabled();
  });

  it("says when the browser cannot use passkeys, without calling Supabase", async () => {
    withoutWebAuthn();
    const { auth } = renderLogin();
    await click("Sign in with a passkey");
    expect(auth.signInWithPasskey).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("This browser can’t use passkeys. Continue with Google instead.");
  });

  it("reads Supabase's own unsupported-browser answer the same way", async () => {
    const auth = stubAuth({ signInWithPasskey: vi.fn().mockResolvedValue({ data: null, error: new AuthUnknownError("Browser does not support WebAuthn", null) }) });
    renderLogin({ auth });
    await click("Sign in with a passkey");
    expect(screen.getByRole("alert")).toHaveTextContent("This browser can’t use passkeys. Continue with Google instead.");
  });

  it("points to Google when passkeys are off on the project (passkey_disabled)", async () => {
    const auth = stubAuth({ signInWithPasskey: vi.fn().mockResolvedValue({ data: null, error: new AuthApiError("Passkey authentication is disabled", 422, "passkey_disabled") }) });
    renderLogin({ auth });
    await click("Sign in with a passkey");
    expect(screen.getByRole("alert")).toHaveTextContent("Passkeys aren’t turned on yet; continue with Google.");
  });

  it("gives one generic message for a passkey the project does not know", async () => {
    const auth = stubAuth({ signInWithPasskey: vi.fn().mockResolvedValue({ data: null, error: new AuthApiError("Credential not found", 404, "webauthn_credential_not_found") }) });
    renderLogin({ auth });
    await click("Sign in with a passkey");
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("That passkey didn’t sign you in. Try again, or continue with Google.");
    expect(alert.textContent).not.toMatch(REVEALING);
  });

  it("shows the callback's failure as one generic line", () => {
    renderLogin({ failed: true });
    expect(screen.getByRole("alert")).toHaveTextContent(SIGN_IN_FAILED);
    expect(screen.getByRole("alert").textContent).not.toMatch(REVEALING);
  });

  it("with sign-in off, says so and calls nothing", () => {
    const { auth } = renderLogin({ enabled: false });
    expect(screen.getByText(/Sign-in is off in this build/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Owlhead" })).toHaveAttribute("href", "/");
    expect(screen.queryByRole("button")).toBeNull();
    for (const fn of Object.values(auth)) expect(fn).not.toHaveBeenCalled();
  });
});

describe("email sign-in, behind its flag", () => {
  async function send(auth: ReturnType<typeof stubAuth>, address = "owner@example.com") {
    renderLogin({ auth, emailEnabled: true });
    fireEvent.change(screen.getByLabelText("Or get a sign-in link by email"), { target: { value: address } });
    await click("Email me a link");
    return document.querySelector("[data-slot=login-email]")?.textContent ?? "";
  }

  it("sends a link back to the callback with next, and never creates an account", async () => {
    const auth = stubAuth();
    const text = await send(auth);
    expect(auth.signInWithOtp).toHaveBeenCalledWith({
      email: "owner@example.com",
      options: { emailRedirectTo: `${window.location.origin}/auth/callback?next=%2Fagents`, shouldCreateUser: false },
    });
    expect(text).toContain(EMAIL_SENT);
    expect(EMAIL_SENT).toBe("If that address can sign in, we’ve sent a link. Open it in this browser to finish.");
  });

  it.each([
    ["no account for the address", new AuthApiError("Signups not allowed for otp", 422, "otp_disabled")],
    ["an unknown user", new AuthApiError("User not found", 400, "user_not_found")],
    ["a rate limit", new AuthApiError("Email rate limit exceeded", 429, "over_email_send_rate_limit")],
    ["an invalid address", new AuthApiError("Unable to validate email address: invalid format", 400, "email_address_invalid")],
  ])("answers exactly as it does for a sent link when Supabase reports %s", async (_what, error) => {
    const sentText = await send(stubAuth());
    document.body.innerHTML = "";
    const errorText = await send(stubAuth({ signInWithOtp: vi.fn().mockResolvedValue({ data: { user: null, session: null }, error }) }));
    expect(errorText).toBe(sentText);
    expect(errorText).not.toMatch(REVEALING);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says only that the service did not answer when the network fails", async () => {
    await send(stubAuth({ signInWithOtp: vi.fn().mockResolvedValue({ data: { user: null, session: null }, error: new AuthRetryableFetchError("Failed to fetch", 0) }) }));
    expect(screen.getByRole("alert")).toHaveTextContent("The sign-in service didn’t answer. Nothing changed; try again in a moment.");
    expect(screen.getByRole("button", { name: "Email me a link" })).toBeEnabled();
  });

  it("does not send an empty address", async () => {
    const auth = stubAuth();
    renderLogin({ auth, emailEnabled: true });
    expect(screen.getByRole("button", { name: "Email me a link" })).toBeDisabled();
    expect(auth.signInWithOtp).not.toHaveBeenCalled();
  });

  it("shows that it is sending", async () => {
    const auth = stubAuth({ signInWithOtp: vi.fn(() => new Promise(() => {})) });
    renderLogin({ auth, emailEnabled: true });
    fireEvent.change(screen.getByLabelText("Or get a sign-in link by email"), { target: { value: "owner@example.com" } });
    await click("Email me a link");
    expect(screen.getByRole("status")).toHaveTextContent("Sending…");
    expect(screen.getByRole("button", { name: "Email me a link" })).toBeDisabled();
  });

  it("lets the owner start again with another address", async () => {
    await send(stubAuth());
    fireEvent.click(screen.getByRole("button", { name: "Use a different address" }));
    expect(screen.getByLabelText("Or get a sign-in link by email")).toHaveValue("");
  });
});
