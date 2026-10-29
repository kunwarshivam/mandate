import { act, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithRuntime } from "@/test/harness";
import { SettingsScreen } from "@/components/agent-chat/settings-screen";

const list = vi.hoisted(() => vi.fn(async () => ({ data: [], error: null })));

vi.mock("@/lib/supabase/client", () => ({
  createClient: () => ({ auth: { registerPasskey: vi.fn(), passkey: { list, update: vi.fn(), delete: vi.fn() } } }),
}));

describe("Settings' profile", () => {
  it("while sign-in is off: no passkeys and no call to Supabase", () => {
    renderWithRuntime(<SettingsScreen />);
    expect(screen.getByText("Sign in to manage your passkeys.")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Passkeys" })).toBeNull();
    expect(list).not.toHaveBeenCalled();
  });

  it("manages passkeys while signed in", async () => {
    await act(async () => {
      renderWithRuntime(<SettingsScreen />, "normal", { session: { email: "ada@example.com" } });
    });
    expect(screen.getByRole("heading", { name: "Passkeys" })).toBeInTheDocument();
    expect(list).toHaveBeenCalledOnce();
  });
});
