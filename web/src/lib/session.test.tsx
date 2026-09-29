import { AuthRetryableFetchError } from "@supabase/supabase-js";
import { describe, expect, it, vi } from "vitest";
import { signOut } from "./session";

describe("signOut", () => {
  it("ends the Supabase session, then goes to /", async () => {
    const order: string[] = [];
    const auth = { signOut: vi.fn(async () => (order.push("signOut"), { error: null })) };
    const navigate = vi.fn((href: string) => order.push(`go ${href}`));
    await signOut(auth, navigate);
    expect(auth.signOut).toHaveBeenCalledOnce();
    expect(order).toEqual(["signOut", "go /"]);
  });

  it("still leaves the app when Supabase answers with an error, since the local session is gone either way", async () => {
    const auth = { signOut: vi.fn(async () => ({ error: new AuthRetryableFetchError("Failed to fetch", 0) })) };
    const navigate = vi.fn();
    await signOut(auth, navigate);
    expect(navigate).toHaveBeenCalledWith("/");
  });
});
