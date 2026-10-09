/**
 * Turning on browser push (E8-14, slice S8d; notifications spec §4.6). On a user's gesture it asks
 * the browser for permission, registers `public/push-sw.js`, subscribes with the deployment's VAPID
 * public key, and hands the subscription to `send`. The subscription is an address (NT-2): it goes
 * only to `send`, never to a log, storage, or the page.
 */

/** Where the worker is served from; its scope is the whole app, so a tap can open `/n/<notice>`. */
export const PUSH_WORKER_PATH = "/push-sw.js";

/** What the workspace API is given: the push endpoint and the two keys to encrypt to. */
export type PushSubscriptionJson = {
  endpoint: string;
  keys: { p256dh: string; auth: string };
};

/** Sends the subscription to the workspace. No API route exists yet, so callers pass a fixture. */
export type SubscriptionSender = (subscription: PushSubscriptionJson) => Promise<void>;

export type SubscribeResult =
  | { kind: "subscribed" }
  | { kind: "unsupported" }
  | { kind: "denied" }
  | { kind: "failed" };

/** The parts of the browser this needs, injected so tests need no real browser. */
export type PushBrowser = {
  serviceWorker?: { register(path: string, options: { scope: string }): Promise<{ pushManager?: PushManagerLike }> };
  requestPermission?: () => Promise<NotificationPermission>;
};

type PushManagerLike = {
  subscribe(options: { userVisibleOnly: true; applicationServerKey: Uint8Array<ArrayBuffer> }): Promise<{ toJSON(): unknown }>;
};

/**
 * The browsers' push services a subscription may point at (DEC-792): an exact host, or `*.` and a
 * domain, which matches a proper subdomain at any depth and never the domain itself.
 */
export const PUSH_SERVICE_HOSTS = [
  "fcm.googleapis.com",
  "updates.push.services.mozilla.com",
  "*.push.apple.com",
  "*.notify.windows.com",
] as const;

/**
 * Whether `endpoint` is `https` on port 443, with no user information, and a lowercase ASCII host
 * on `PUSH_SERVICE_HOSTS`; an IP literal, a trailing dot, an IDN label, or percent-encoding in the
 * host never matches (notifications spec §4.6).
 */
export function allowedPushEndpoint(endpoint: string): boolean {
  void endpoint;
  throw new Error("Unimplemented: E8-14");
}

/** The VAPID public key as the 65 octets of an uncompressed P-256 point, or null. */
export function vapidKeyBytes(key: string): Uint8Array<ArrayBuffer> | null {
  void key;
  throw new Error("Unimplemented: E8-14");
}

/** The subscription's endpoint and keys, or null when the browser gave anything else. */
export function subscriptionJson(value: unknown): PushSubscriptionJson | null {
  void value;
  throw new Error("Unimplemented: E8-14");
}

export async function subscribeToPush(
  vapidPublicKey: string,
  send: SubscriptionSender,
  browser: PushBrowser,
): Promise<SubscribeResult> {
  void [vapidPublicKey, send, browser];
  throw new Error("Unimplemented: E8-14");
}

/** The deployment's VAPID public key, inlined at build time; empty when push is not set up. */
export const VAPID_PUBLIC_KEY = process.env.NEXT_PUBLIC_OWLHEAD_VAPID_PUBLIC_KEY?.trim() ?? "";

/**
 * A fixture sender (DEC-200 item 6): no workspace API route takes a subscription yet, so this keeps
 * nothing and sends nothing. The screen that uses it says so.
 */
export const fixtureSender: SubscriptionSender = async () => {};
