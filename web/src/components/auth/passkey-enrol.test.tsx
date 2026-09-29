import { AuthApiError, AuthSessionMissingError } from "@supabase/supabase-js";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type EnrolAuth, PasskeyEnrol } from "./passkey-enrol";

beforeEach(() => {
  Object.defineProperty(window, "PublicKeyCredential", { configurable: true, writable: true, value: function PublicKeyCredential() {} });
  Object.defineProperty(navigator, "credentials", { configurable: true, value: { get: vi.fn(), create: vi.fn() } });
});

afterEach(() => {
  Reflect.deleteProperty(window, "PublicKeyCredential");
  Reflect.deleteProperty(navigator, "credentials");
});

function renderEnrol(registerPasskey = vi.fn().mockResolvedValue({ data: { id: "pk_1", created_at: "2026-09-29T08:00:00Z" }, error: null })) {
  const navigate = vi.fn();
  render(<PasskeyEnrol next="/agents" enabled auth={{ registerPasskey } as unknown as EnrolAuth} navigate={navigate} />);
  return { registerPasskey, navigate };
}

const add = async () => {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Add a passkey" }));
  });
};

describe("the passkey step after a first sign-in", () => {
  it("offers to add a passkey, with one line on why, and lets the owner skip to next", () => {
    renderEnrol();
    expect(screen.getByRole("heading", { level: 1, name: "Add a passkey" })).toBeInTheDocument();
    expect(screen.getByText("Next time, sign in with your fingerprint, face or device PIN instead of Google.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Not now" })).toHaveAttribute("href", "/agents");
  });

  it("goes on to next once the passkey is added", async () => {
    const { registerPasskey, navigate } = renderEnrol();
    await add();
    expect(registerPasskey).toHaveBeenCalledOnce();
    expect(navigate).toHaveBeenCalledWith("/agents");
  });

  it.each([
    [Object.assign(new Error("not allowed"), { name: "NotAllowedError", code: "ERROR_PASSTHROUGH_SEE_CAUSE_PROPERTY" }), "No passkey was added. The request was cancelled or timed out."],
    [new AuthApiError("Passkeys are disabled", 422, "passkey_disabled"), "Passkeys aren’t turned on yet."],
    [new AuthApiError("Credential exists", 422, "webauthn_credential_exists"), "This device’s passkey is already on your account."],
    [new AuthSessionMissingError(), "Your sign-in has ended. Sign in again to add a passkey."],
  ])("says what happened and stays, so the owner can still skip (%s)", async (error, text) => {
    const { navigate } = renderEnrol(vi.fn().mockResolvedValue({ data: null, error }));
    await add();
    expect(screen.getByRole("alert")).toHaveTextContent(text);
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("link", { name: "Not now" })).toBeInTheDocument();
  });

  it("says when the browser cannot create a passkey, without calling Supabase", async () => {
    Reflect.deleteProperty(window, "PublicKeyCredential");
    const { registerPasskey } = renderEnrol();
    await add();
    expect(registerPasskey).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("This browser can’t create a passkey.");
  });
});
