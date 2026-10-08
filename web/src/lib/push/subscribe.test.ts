import { describe, expect, it, vi } from "vitest";
import { PUSH_WORKER_PATH, type PushBrowser, subscribeToPush, subscriptionJson, vapidKeyBytes } from "./subscribe";

/** E8-14 S8d: permission, registration and subscription, with a fake browser (notifications spec §4.6). */

const VAPID = "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8";
const SUBSCRIPTION = {
  endpoint: "https://updates.push.services.mozilla.com/wpush/v2/abc",
  keys: {
    p256dh: "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8",
    auth: "BTBZMqHH6r4Tts7J_aSIgg",
  },
};

function browser(permission: NotificationPermission, json: unknown = { ...SUBSCRIPTION, expirationTime: null }) {
  const subscribe = vi.fn(async () => ({ toJSON: () => json }));
  const register = vi.fn(async () => ({ pushManager: { subscribe } }));
  const fake: PushBrowser = { serviceWorker: { register }, requestPermission: async () => permission };
  return { fake, register, subscribe };
}

describe("subscribing to push", () => {
  it.skip("pending E8-14: registers the worker for the whole app, subscribes with the VAPID key, and sends only endpoint and keys", async () => {
    const send = vi.fn(async () => {});
    const { fake, register, subscribe } = browser("granted");
    expect(await subscribeToPush(VAPID, send, fake)).toEqual({ kind: "subscribed" });
    expect(register).toHaveBeenCalledWith(PUSH_WORKER_PATH, { scope: "/" });
    expect(subscribe).toHaveBeenCalledWith({ userVisibleOnly: true, applicationServerKey: vapidKeyBytes(VAPID) });
    expect(send).toHaveBeenCalledWith(SUBSCRIPTION);
  });

  it.skip("pending E8-14: sends nothing when permission is refused or dismissed", async () => {
    for (const permission of ["denied", "default"] as const) {
      const send = vi.fn(async () => {});
      const { fake, register } = browser(permission);
      expect(await subscribeToPush(VAPID, send, fake)).toEqual({ kind: "denied" });
      expect(register).not.toHaveBeenCalled();
      expect(send).not.toHaveBeenCalled();
    }
  });

  it.skip("pending E8-14: is unsupported without a service worker, a permission prompt, a push manager, or a valid key", async () => {
    const send = vi.fn(async () => {});
    const { fake } = browser("granted");
    expect(await subscribeToPush(VAPID, send, { requestPermission: fake.requestPermission })).toEqual({ kind: "unsupported" });
    expect(await subscribeToPush(VAPID, send, { serviceWorker: fake.serviceWorker })).toEqual({ kind: "unsupported" });
    const bare: PushBrowser = { ...fake, serviceWorker: { register: async () => ({}) } };
    expect(await subscribeToPush(VAPID, send, bare)).toEqual({ kind: "unsupported" });
    for (const key of ["", VAPID.slice(1), `A${VAPID.slice(1)}`, `${VAPID.slice(0, 86)}+`]) {
      expect(await subscribeToPush(key, send, fake), key).toEqual({ kind: "unsupported" });
    }
    expect(send).not.toHaveBeenCalled();
  });

  it.skip("pending E8-14: fails without sending when the browser's subscription is malformed or a step throws", async () => {
    const send = vi.fn(async () => {});
    for (const json of [null, { endpoint: "http://push.example/x", keys: SUBSCRIPTION.keys }, { endpoint: SUBSCRIPTION.endpoint }, { endpoint: SUBSCRIPTION.endpoint, keys: { p256dh: "a b", auth: "x" } }]) {
      expect(await subscribeToPush(VAPID, send, browser("granted", json).fake)).toEqual({ kind: "failed" });
    }
    expect(send).not.toHaveBeenCalled();
    const throwing: PushBrowser = { ...browser("granted").fake, requestPermission: async () => Promise.reject(new Error("x")) };
    expect(await subscribeToPush(VAPID, send, throwing)).toEqual({ kind: "failed" });
    expect(await subscribeToPush(VAPID, async () => Promise.reject(new Error("api down")), browser("granted").fake)).toEqual({ kind: "failed" });
  });

  it.skip("pending E8-14: reads the key as the 65 octets of an uncompressed point and the subscription as endpoint and keys only", () => {
    const bytes = vapidKeyBytes(VAPID);
    expect(bytes?.length).toBe(65);
    expect(bytes?.[0]).toBe(4);
    expect(subscriptionJson({ ...SUBSCRIPTION, expirationTime: 1, extra: "x" })).toEqual(SUBSCRIPTION);
  });
});
