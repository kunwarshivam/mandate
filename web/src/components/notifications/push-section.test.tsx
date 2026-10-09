import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PushBrowser } from "@/lib/push/subscribe";
import { PUSH_COPY, PushSection } from "./push-section";

/** E8-14 S8d: the Settings › Notifications control (notifications spec §4.6; DEC-200 item 6). */

const VAPID = "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8";
const SUBSCRIPTION = {
  endpoint: "https://updates.push.services.mozilla.com/wpush/v2/abc",
  keys: { p256dh: "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8", auth: "BTBZMqHH6r4Tts7J_aSIgg" },
};

function browser(permission: NotificationPermission): PushBrowser {
  return {
    requestPermission: async () => permission,
    serviceWorker: { register: async () => ({ pushManager: { subscribe: async () => ({ toJSON: () => SUBSCRIPTION }) } }) },
  };
}

describe("PushSection", () => {
  it.skip("pending E8-14: turns on notifications and says so, sending the subscription to the given sender", async () => {
    const send = vi.fn(async () => {});
    render(<PushSection vapidPublicKey={VAPID} send={send} browser={browser("granted")} />);
    fireEvent.click(screen.getByRole("button", { name: "Turn on notifications" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(PUSH_COPY.subscribed));
    expect(send).toHaveBeenCalledWith(SUBSCRIPTION);
    expect(screen.queryByText(/Fixture/)).toBeNull();
  });

  it.skip("pending E8-14: says what to do when the browser blocks notifications, and sends nothing", async () => {
    const send = vi.fn(async () => {});
    render(<PushSection vapidPublicKey={VAPID} send={send} browser={browser("denied")} />);
    fireEvent.click(screen.getByRole("button", { name: "Turn on notifications" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(PUSH_COPY.denied));
    expect(send).not.toHaveBeenCalled();
  });

  it.skip("pending E8-14: labels the fixture sender as not live (DEC-200 item 6)", () => {
    render(<PushSection vapidPublicKey={VAPID} browser={browser("granted")} />);
    expect(screen.getByText("Fixture: this build does not send the subscription anywhere yet.")).toBeInTheDocument();
  });

  it.skip("pending E8-14: is off without the deployment's VAPID key, and never asks the browser", () => {
    const requestPermission = vi.fn(async () => "granted" as const);
    const register = vi.fn(async () => ({}));
    const send = vi.fn(async () => {});
    render(<PushSection vapidPublicKey="" send={send} browser={{ requestPermission, serviceWorker: { register } }} />);
    const button = screen.getByRole("button", { name: "Turn on notifications" });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(screen.getByText("Browser notifications aren’t set up for this deployment.")).toBeInTheDocument();
    expect(requestPermission).not.toHaveBeenCalled();
    expect(register).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
  });

  it("says only generic things: no instrument, amount, or agent in any copy (rule 6)", () => {
    for (const text of Object.values(PUSH_COPY)) {
      expect(text).not.toMatch(/\$|\d|buy|sell|order|position|agent/i);
    }
  });
});
