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

const BASE64URL = /^[A-Za-z0-9_-]+$/;

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

const ENDPOINT = /^https:\/\/([a-z0-9.-]+)(?::443)?(?:\/\S*)?$/;

/**
 * Whether `endpoint` is `https` on port 443, with no user information, and a lowercase ASCII host
 * on `PUSH_SERVICE_HOSTS` (notifications spec §4.6). An empty, hyphen-edged, or IDN (`xn--`) label
 * is refused; a bracketed address or percent-encoding never parses as a host, and an IPv4 literal
 * can never equal or end in an entry, since every entry is a name.
 */
export function allowedPushEndpoint(endpoint: string): boolean {
  const host = ENDPOINT.exec(endpoint)?.[1];
  if (host === undefined) return false;
  const labels = host.split(".");
  if (labels.some((label) => label === "" || label.startsWith("xn--") || label.startsWith("-") || label.endsWith("-"))) return false;
  return PUSH_SERVICE_HOSTS.some((entry) => (entry.startsWith("*.") ? host.endsWith(entry.slice(1)) : host === entry));
}

/** The VAPID public key as the 65 octets of an uncompressed P-256 point, or null. */
export function vapidKeyBytes(key: string): Uint8Array<ArrayBuffer> | null {
  if (key.length !== 87 || !BASE64URL.test(key)) return null;
  const binary = atob(key.replaceAll("-", "+").replaceAll("_", "/") + "=");
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes.length === 65 && bytes[0] === 4 ? bytes : null;
}

/** The subscription's endpoint and keys, or null when the browser gave anything else. */
export function subscriptionJson(value: unknown): PushSubscriptionJson | null {
  if (typeof value !== "object" || value === null) return null;
  const { endpoint, keys } = value as { endpoint?: unknown; keys?: unknown };
  if (typeof endpoint !== "string" || !allowedPushEndpoint(endpoint)) return null;
  if (typeof keys !== "object" || keys === null) return null;
  const { p256dh, auth } = keys as { p256dh?: unknown; auth?: unknown };
  if (typeof p256dh !== "string" || !BASE64URL.test(p256dh)) return null;
  if (typeof auth !== "string" || !BASE64URL.test(auth)) return null;
  return { endpoint, keys: { p256dh, auth } };
}

export async function subscribeToPush(
  vapidPublicKey: string,
  send: SubscriptionSender,
  browser: PushBrowser,
): Promise<SubscribeResult> {
  const key = vapidKeyBytes(vapidPublicKey);
  if (key === null || !browser.serviceWorker || !browser.requestPermission) return { kind: "unsupported" };
  try {
    if ((await browser.requestPermission()) !== "granted") return { kind: "denied" };
    const registration = await browser.serviceWorker.register(PUSH_WORKER_PATH, { scope: "/" });
    if (!registration.pushManager) return { kind: "unsupported" };
    const subscription = await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
    const json = subscriptionJson(subscription.toJSON());
    if (json === null) return { kind: "failed" };
    await send(json);
    return { kind: "subscribed" };
  } catch {
    return { kind: "failed" };
  }
}

/** The deployment's VAPID public key, inlined at build time; empty when push is not set up. */
export const VAPID_PUBLIC_KEY = process.env.NEXT_PUBLIC_OWLHEAD_VAPID_PUBLIC_KEY?.trim() ?? "";

/**
 * A fixture sender (DEC-200 item 6): no workspace API route takes a subscription yet, so this keeps
 * nothing and sends nothing. The screen that uses it says so.
 */
export const fixtureSender: SubscriptionSender = async () => {};
