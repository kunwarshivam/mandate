"use client";

import { useState } from "react";
import { Section } from "@/components/screens/common";
import { NOTICE } from "@/components/auth/buttons";
import { KEY_SM } from "@/components/kumo/key";
import {
  fixtureSender,
  type PushBrowser,
  type SubscribeResult,
  type SubscriptionSender,
  subscribeToPush,
  VAPID_PUBLIC_KEY,
} from "@/lib/push/subscribe";

/** What each outcome tells the person; generic, as every notification is (rule 6). */
export const PUSH_COPY: Record<SubscribeResult["kind"], string> = {
  subscribed: "This browser will now tell you when something needs you.",
  denied: "Notifications are blocked for this site. Allow them in your browser’s settings, then try again.",
  unsupported: "This browser can’t receive notifications from Owlhead.",
  failed: "Notifications couldn’t be turned on. Nothing changed; try again.",
};

/** The browser's own push parts, read only when the person asks. */
function realBrowser(): PushBrowser {
  return {
    serviceWorker: typeof navigator !== "undefined" && "serviceWorker" in navigator ? navigator.serviceWorker : undefined,
    requestPermission:
      typeof Notification !== "undefined" && typeof window !== "undefined" && "PushManager" in window
        ? () => Notification.requestPermission()
        : undefined,
  };
}

/**
 * Turning on browser notifications (E8-14, notifications spec §4.6). A notification says only that
 * an approval, an alert, an account change, or the daily brief is waiting; the details open in
 * Owlhead after sign-in. Until the workspace API takes a subscription, `send` is a fixture and the
 * section says so.
 */
export function PushSection({
  vapidPublicKey = VAPID_PUBLIC_KEY,
  send = fixtureSender,
  browser,
}: {
  vapidPublicKey?: string;
  send?: SubscriptionSender;
  browser?: PushBrowser;
}) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const configured = vapidPublicKey !== "";

  async function turnOn() {
    setBusy(true);
    setMessage(null);
    const result = await subscribeToPush(vapidPublicKey, send, browser ?? realBrowser());
    setBusy(false);
    setMessage(PUSH_COPY[result.kind]);
  }

  return (
    <Section
      title="Browser notifications"
      className="max-w-3xl"
      action={
        <button type="button" onClick={() => void turnOn()} disabled={busy || !configured} className={KEY_SM}>
          Turn on notifications
        </button>
      }
    >
      <div className="grid gap-(--block-gap)">
        {send === fixtureSender ? <p className={NOTICE}>Fixture: this build does not send the subscription anywhere yet.</p> : null}
        {configured ? null : <p className={NOTICE}>Browser notifications aren’t set up for this deployment.</p>}
        <p role="status" aria-live="polite" className={message ? NOTICE : "sr-only"}>
          {message ?? ""}
        </p>
      </div>
    </Section>
  );
}
