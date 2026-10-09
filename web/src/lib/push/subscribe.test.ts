import { describe, expect, it, vi } from "vitest";
import { afterEach, beforeEach } from "vitest";
import { PUSH_WORKER_PATH, type PushBrowser, allowedPushEndpoint, subscribeToPush, subscriptionJson, vapidKeyBytes } from "./subscribe";

/**
 * E8-14 S8d: permission, registration and subscription, with a fake browser (notifications spec
 * §4.6). The allowlist table is DEC-792's, as notifications spec §4.6 states it; both are in #827.
 */

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
  it("registers the worker for the whole app, subscribes with the VAPID key, and sends only endpoint and keys", async () => {
    const send = vi.fn(async () => {});
    const { fake, register, subscribe } = browser("granted");
    expect(await subscribeToPush(VAPID, send, fake)).toEqual({ kind: "subscribed" });
    expect(register).toHaveBeenCalledWith(PUSH_WORKER_PATH, { scope: "/" });
    expect(subscribe).toHaveBeenCalledWith({ userVisibleOnly: true, applicationServerKey: vapidKeyBytes(VAPID) });
    expect(send).toHaveBeenCalledWith(SUBSCRIPTION);
  });

  it("sends nothing when permission is refused or dismissed", async () => {
    for (const permission of ["denied", "default"] as const) {
      const send = vi.fn(async () => {});
      const { fake, register } = browser(permission);
      expect(await subscribeToPush(VAPID, send, fake)).toEqual({ kind: "denied" });
      expect(register).not.toHaveBeenCalled();
      expect(send).not.toHaveBeenCalled();
    }
  });

  it("is unsupported without a service worker, a permission prompt, a push manager, or a valid key", async () => {
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

  it("fails without sending when the browser's subscription is malformed or a step throws", async () => {
    const send = vi.fn(async () => {});
    for (const json of [null, { endpoint: "http://push.example/x", keys: SUBSCRIPTION.keys }, { endpoint: SUBSCRIPTION.endpoint }, { endpoint: SUBSCRIPTION.endpoint, keys: { p256dh: "a b", auth: "x" } }]) {
      expect(await subscribeToPush(VAPID, send, browser("granted", json).fake)).toEqual({ kind: "failed" });
    }
    expect(send).not.toHaveBeenCalled();
    const throwing: PushBrowser = { ...browser("granted").fake, requestPermission: async () => Promise.reject(new Error("x")) };
    expect(await subscribeToPush(VAPID, send, throwing)).toEqual({ kind: "failed" });
    expect(await subscribeToPush(VAPID, async () => Promise.reject(new Error("api down")), browser("granted").fake)).toEqual({ kind: "failed" });
  });

  it("reads the key as the 65 octets of an uncompressed point and the subscription as endpoint and keys only", () => {
    const bytes = vapidKeyBytes(VAPID);
    expect(bytes?.length).toBe(65);
    expect(bytes?.[0]).toBe(4);
    expect(subscriptionJson({ ...SUBSCRIPTION, expirationTime: 1, extra: "x" })).toEqual(SUBSCRIPTION);
  });

  it("accepts an endpoint only on the browsers' push services, and sends nothing otherwise (DEC-792)", async () => {
    const allowed = [
      "https://fcm.googleapis.com/fcm/send/abc",
      "https://fcm.googleapis.com:443/fcm/send/abc",
      "https://updates.push.services.mozilla.com/wpush/v2/abc",
      "https://web.push.apple.com/QGv",
      "https://a.b.push.apple.com/QGv",
      "https://wns2-x.notify.windows.com/w/?token=abc",
      "https://wns2-xyz.notify.windows.com/w/?token=abc",
    ];
    const refused = [
      "https://evil.example/",
      "https://fcm.googleapis.com.evil.example/x",
      "https://evilfcm.googleapis.com/x",
      "https://push.apple.com.evil.example/x",
      "https://push.apple.com/x",
      "https://notify.windows.com/x",
      "https://evilpush.apple.com/x",
      "https://fcm.googleapis.com:8443/x",
      "https://u:p@fcm.googleapis.com/x",
      "https://fcm.googleapis.com@evil.example/x",
      "https://1.2.3.4/x",
      "https://[::1]/x",
      "https://android.googleapis.com/x",
      "http://fcm.googleapis.com/x",
      "https://FCM.googleapis.com/x",
      "https://WEB.push.apple.com/x",
      "https://wns2-X.notify.windows.com/x",
      "https://fcm.googleapis.com./x",
      "https://xn--fcm-0na.googleapis.com/x",
      "https://fcm%2Egoogleapis.com/x",
      "https://fcm.googleapis.com\\@evil.example/x",
      " https://fcm.googleapis.com/x",
      "https://xn--web-0na.push.apple.com/x",
      "https://a..push.apple.com/x",
      "https://-a.push.apple.com/x",
      "https://a-.notify.windows.com/x",
    ];
    for (const endpoint of allowed) {
      expect(allowedPushEndpoint(endpoint), endpoint).toBe(true);
      const send = vi.fn(async () => {});
      expect(await subscribeToPush(VAPID, send, browser("granted", { endpoint, keys: SUBSCRIPTION.keys }).fake), endpoint).toEqual({ kind: "subscribed" });
      expect(send).toHaveBeenCalledWith({ endpoint, keys: SUBSCRIPTION.keys });
    }
    for (const endpoint of refused) {
      expect(allowedPushEndpoint(endpoint), endpoint).toBe(false);
      expect(subscriptionJson({ endpoint, keys: SUBSCRIPTION.keys }), endpoint).toBeNull();
      const send = vi.fn(async () => {});
      expect(await subscribeToPush(VAPID, send, browser("granted", { endpoint, keys: SUBSCRIPTION.keys }).fake), endpoint).toEqual({ kind: "failed" });
      expect(send).not.toHaveBeenCalled();
    }
  });
});

describe("the subscription stays out of the console (NT-2)", () => {
  const methods = ["log", "info", "warn", "error", "debug", "trace"] as const;
  let spies: ReturnType<typeof vi.spyOn>[] = [];
  beforeEach(() => {
    spies = methods.map((m) => vi.spyOn(console, m).mockImplementation(() => {}));
  });
  afterEach(() => spies.forEach((spy) => spy.mockRestore()));

  it("writes no endpoint or key to the console on any path", async () => {
    const leaky = new Error(`push failed for ${SUBSCRIPTION.endpoint} ${SUBSCRIPTION.keys.auth}`);
    await subscribeToPush(VAPID, async () => {}, browser("granted").fake);
    await subscribeToPush(VAPID, async () => {}, browser("granted", { endpoint: SUBSCRIPTION.endpoint, keys: { p256dh: "a b", auth: SUBSCRIPTION.keys.auth } }).fake);
    await subscribeToPush(VAPID, async () => Promise.reject(leaky), browser("granted").fake);
    const throwing: PushBrowser = { ...browser("granted").fake, serviceWorker: { register: async () => Promise.reject(leaky) } };
    await subscribeToPush(VAPID, async () => {}, throwing);
    const written = spies.flatMap((spy) => spy.mock.calls.map((args: unknown[]) => args.map((a: unknown) => String(a instanceof Error ? `${a.message}${a.stack}` : JSON.stringify(a))).join(" ")));
    for (const secret of ["push.services.mozilla.com", SUBSCRIPTION.keys.p256dh, SUBSCRIPTION.keys.auth]) {
      expect(written.filter((line) => line.includes(secret))).toEqual([]);
    }
  });
});
