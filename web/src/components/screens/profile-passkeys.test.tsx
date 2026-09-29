import { act, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { findScreen } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { RegistryScreen } from "./account-screens";

const list = vi.hoisted(() => vi.fn(async () => ({ data: [], error: null })));

vi.mock("@/lib/supabase/client", () => ({
  createClient: () => ({ auth: { registerPasskey: vi.fn(), passkey: { list, update: vi.fn(), delete: vi.fn() } } }),
}));

const profile = findScreen("/settings/profile")!;

describe("the Profile screen", () => {
  it("reads as before while sign-in is off: coming next, no passkeys and no call to Supabase", () => {
    renderWithRuntime(<RegistryScreen screen={profile} />);
    expect(screen.getByText(profile.purpose)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Passkeys" })).toBeNull();
    expect(list).not.toHaveBeenCalled();
  });

  it("manages passkeys while signed in, and still says what else is coming", async () => {
    await act(async () => {
      renderWithRuntime(<RegistryScreen screen={profile} />, "normal", { session: { email: "ada@example.com" } });
    });
    expect(screen.getByRole("heading", { level: 2, name: "Passkeys" })).toBeInTheDocument();
    expect(list).toHaveBeenCalledOnce();
    expect(screen.getByText("Your name and sign-in sessions.")).toBeInTheDocument();
  });
});
