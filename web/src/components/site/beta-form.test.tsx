import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as AuthConfig from "@/lib/auth-config";
import { betaOpen } from "@/lib/beta";
import { BetaForm } from "./beta-form";

const supabase = vi.hoisted(() => ({ url: "", key: "" }));

vi.mock("@/lib/auth-config", async (original) => ({
  ...(await original<typeof AuthConfig>()),
  get SUPABASE_URL() {
    return supabase.url;
  },
  get SUPABASE_PUBLISHABLE_KEY() {
    return supabase.key;
  },
}));

beforeEach(() => {
  supabase.url = "";
  supabase.key = "";
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

describe("whether the private beta takes requests (DEC-987)", () => {
  it("pending E21-38: is open in development, the tests and the e2e builds, and elsewhere only with both Supabase values", () => {
    for (const nodeEnv of ["development", "test"]) expect(betaOpen(nodeEnv, "", "", undefined), nodeEnv).toBe(true);
    expect(betaOpen("production", "", "", "1")).toBe(true);
    expect(betaOpen("production", "https://stub-project.supabase.co", "sb_publishable_stub", undefined)).toBe(true);
    for (const [url, key] of [
      ["", ""],
      ["https://stub-project.supabase.co", ""],
      ["", "sb_publishable_stub"],
      ["  ", "sb_publishable_stub"],
      ["https://stub-project.supabase.co", " "],
    ]) {
      for (const nodeEnv of ["production", undefined, "", "Production"]) expect(betaOpen(nodeEnv, url, key, "0"), `${nodeEnv} ${url} ${key}`).toBe(false);
    }
  });
});

describe("the beta form when the beta is closed (DEC-987)", () => {
  it("pending E21-38: says it isn't taking requests and offers no field to send, in both looks", () => {
    vi.stubEnv("NODE_ENV", "production");
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    for (const look of ["guestbook", "page"] as const) {
      const { container, unmount } = render(<BetaForm look={look} />);
      expect(container.querySelector("[data-slot=beta-closed]"), look).toHaveTextContent("The private beta isn't taking requests yet.");
      expect(container.querySelector("form, input, button"), look).toBeNull();
      unmount();
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("pending E21-38: shows the form in a production build once Supabase is configured", () => {
    vi.stubEnv("NODE_ENV", "production");
    supabase.url = "https://stub-project.supabase.co";
    supabase.key = "sb_publishable_stub";
    const { container } = render(<BetaForm />);
    expect(container.querySelector("[data-slot=beta-form]")).not.toBeNull();
    expect(container.querySelector("[data-slot=beta-closed]")).toBeNull();
  });

  it("pending E21-38: turns closed when the route answers closed, and says try again for any other failure", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "closed" }), { status: 403 })));
    const { container, unmount } = render(<BetaForm />);
    fireEvent.change(screen.getByLabelText("Email address:"), { target: { value: "ada@example.com" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign the guestbook" }));
    });
    expect(container.querySelector("[data-slot=beta-closed]")).not.toBeNull();
    expect(container.querySelector("form")).toBeNull();
    unmount();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("forbidden", { status: 403 })));
    const again = render(<BetaForm />);
    fireEvent.change(screen.getByLabelText("Email address:"), { target: { value: "ada@example.com" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign the guestbook" }));
    });
    expect(again.container.querySelector("[data-slot=beta-closed]")).toBeNull();
    expect(document.getElementById("beta-problem")).toHaveTextContent("We couldn't save that just now.");
  });
});
