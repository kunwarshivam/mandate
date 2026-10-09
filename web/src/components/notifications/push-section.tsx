"use client";

import type { PushBrowser, SubscribeResult, SubscriptionSender } from "@/lib/push/subscribe";

/** What each outcome tells the person; generic, as every notification is (rule 6). */
export const PUSH_COPY: Record<SubscribeResult["kind"], string> = {
  subscribed: "This browser will now tell you when something needs you.",
  denied: "Notifications are blocked for this site. Allow them in your browser’s settings, then try again.",
  unsupported: "This browser can’t receive notifications from Owlhead.",
  failed: "Notifications couldn’t be turned on. Nothing changed; try again.",
};

/**
 * Turning on browser notifications (E8-14, notifications spec §4.6). Stub: E8-14 renders the
 * control, its generic copy, and the fixture label (DEC-200 item 6).
 */
export function PushSection(props: { vapidPublicKey?: string; send?: SubscriptionSender; browser?: PushBrowser }): never {
  void props;
  throw new Error("Unimplemented: E8-14");
}
