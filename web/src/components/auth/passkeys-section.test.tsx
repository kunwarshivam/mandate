import type { PasskeyListItem } from "@supabase/supabase-js";
import { AuthApiError } from "@supabase/supabase-js";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NAME_MAX, type PasskeysAuth, PasskeysSection, passkeyTime } from "./passkeys-section";

const LAPTOP: PasskeyListItem = { id: "pk_1", friendly_name: "Laptop", created_at: "2026-09-01T08:30:00Z", last_used_at: "2026-09-28T17:05:12Z" };
const PHONE: PasskeyListItem = { id: "pk_2", created_at: "2026-09-20T10:00:00Z" };

type Api = PasskeysAuth["passkey"];

function stubAuth(list: PasskeyListItem[] = [LAPTOP, PHONE]) {
  let current = [...list];
  return {
    registerPasskey: vi.fn<PasskeysAuth["registerPasskey"]>(async () => {
      current = [...current, { id: "pk_new", friendly_name: "New", created_at: "2026-09-29T09:00:00Z" }];
      return { data: { id: "pk_new", created_at: "2026-09-29T09:00:00Z" }, error: null };
    }),
    passkey: {
      list: vi.fn<Api["list"]>(async () => ({ data: current, error: null })),
      update: vi.fn<Api["update"]>(async ({ passkeyId, friendlyName }) => {
        const found = current.find((p) => p.id === passkeyId)!;
        return { data: { ...found, friendly_name: friendlyName }, error: null };
      }),
      delete: vi.fn<Api["delete"]>(async ({ passkeyId }) => {
        current = current.filter((p) => p.id !== passkeyId);
        return { data: null, error: null };
      }),
    },
  };
}

async function renderSection(auth = stubAuth()) {
  await act(async () => {
    render(<PasskeysSection auth={auth as unknown as PasskeysAuth} />);
  });
  return auth;
}

/** Supabase does not export its WebAuthnError; this carries the name and code it passes on from the browser. */
function webAuthnCancel() {
  return Object.assign(new AuthApiError("The operation either timed out or was not allowed.", 0, undefined), {
    name: "NotAllowedError",
    code: "ERROR_PASSTHROUGH_SEE_CAUSE_PROPERTY",
  });
}

const rows = () => screen.getAllByRole("listitem").filter((li) => li.dataset.slot === "passkey");

beforeEach(() => {
  Object.defineProperty(window, "PublicKeyCredential", { configurable: true, writable: true, value: function PublicKeyCredential() {} });
  Object.defineProperty(navigator, "credentials", { configurable: true, value: { get: vi.fn(), create: vi.fn() } });
});

afterEach(() => {
  Reflect.deleteProperty(window, "PublicKeyCredential");
  Reflect.deleteProperty(navigator, "credentials");
});

describe("the profile's passkeys", () => {
  it("lists each passkey with when it was added and last used", async () => {
    await renderSection();
    expect(screen.getByRole("heading", { level: 2, name: "Passkeys" })).toBeInTheDocument();
    const [laptop, phone] = rows();
    expect(laptop).toHaveTextContent("Laptop");
    expect(laptop).toHaveTextContent("Added Sep 1, 2026, 08:30 UTC. Last used Sep 28, 2026, 17:05 UTC.");
    expect(phone).toHaveTextContent("Passkey");
    expect(phone).toHaveTextContent("Not used yet.");
  });

  it("writes times from the timestamp itself", () => {
    expect(passkeyTime("2026-09-29T09:14:03.123456Z")).toBe("Sep 29, 2026, 09:14 UTC");
  });

  it("says when there are none", async () => {
    await renderSection(stubAuth([]));
    expect(screen.getByText("No passkeys yet. Add one to sign in without Google next time.")).toBeInTheDocument();
  });

  it("offers a retry when the list cannot load", async () => {
    const auth = stubAuth();
    auth.passkey.list.mockResolvedValueOnce({ data: null, error: new AuthApiError("boom", 500, "unexpected_failure") });
    await renderSection(auth);
    expect(screen.getByText("Your passkeys couldn’t be loaded. Nothing changed; try again.")).toBeInTheDocument();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Try again" })));
    expect(rows()).toHaveLength(2);
  });

  it("adds a passkey and shows the new list", async () => {
    const auth = await renderSection();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Add a passkey" })));
    expect(auth.registerPasskey).toHaveBeenCalledOnce();
    expect(rows()).toHaveLength(3);
    expect(screen.getByRole("status")).toHaveTextContent("Passkey added.");
  });

  it("says a cancelled or refused add changed nothing", async () => {
    const auth = stubAuth();
    auth.registerPasskey.mockResolvedValueOnce({ data: null, error: webAuthnCancel() });
    await renderSection(auth);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Add a passkey" })));
    expect(screen.getByRole("status")).toHaveTextContent("No passkey was added. The request was cancelled or timed out.");
    expect(rows()).toHaveLength(2);
  });

  it("renames a passkey, trimmed and within Supabase's 120 characters", async () => {
    const auth = await renderSection();
    fireEvent.click(screen.getByRole("button", { name: "Rename Laptop" }));
    const field = screen.getByRole("textbox", { name: "Name" });
    expect(field).toHaveAttribute("maxlength", String(NAME_MAX));
    fireEvent.change(field, { target: { value: "  Work laptop  " } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save" })));
    expect(auth.passkey.update).toHaveBeenCalledWith({ passkeyId: "pk_1", friendlyName: "Work laptop" });
    expect(rows()[0]).toHaveTextContent("Work laptop");
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("won't save an empty name", async () => {
    const auth = await renderSection();
    fireEvent.click(screen.getByRole("button", { name: "Rename Laptop" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "   " } });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(auth.passkey.update).not.toHaveBeenCalled();
  });

  it("asks before deleting, and deletes only on confirmation", async () => {
    const auth = await renderSection();
    fireEvent.click(screen.getByRole("button", { name: "Delete Laptop" }));
    const confirm = screen.getByRole("group", { name: "Delete Laptop" });
    fireEvent.click(within(confirm).getByRole("button", { name: "Keep it" }));
    expect(auth.passkey.delete).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Delete Laptop" }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Delete passkey" })));
    expect(auth.passkey.delete).toHaveBeenCalledWith({ passkeyId: "pk_1" });
    expect(rows()).toHaveLength(1);
    expect(screen.getByRole("status")).toHaveTextContent("Passkey deleted.");
  });

  it("says Google stays the way in before the last passkey goes", async () => {
    await renderSection(stubAuth([LAPTOP]));
    fireEvent.click(screen.getByRole("button", { name: "Delete Laptop" }));
    expect(screen.getByRole("group", { name: "Delete Laptop" })).toHaveTextContent("Google will be the only way in.");
  });

  it("keeps the passkey and says so when a change fails", async () => {
    const auth = stubAuth();
    auth.passkey.delete.mockResolvedValueOnce({ data: null, error: new AuthApiError("boom", 500, "unexpected_failure") });
    await renderSection(auth);
    fireEvent.click(screen.getByRole("button", { name: "Delete Laptop" }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Delete passkey" })));
    expect(rows()).toHaveLength(2);
    expect(screen.getByRole("status")).toHaveTextContent("That change didn’t save. Nothing changed; try again.");
  });
});
