/*
 * Owlhead's push service worker (E8-14, slice S8d; notifications spec §4.2, §4.3, §4.6).
 *
 * It shows only one of four fixed sentences and, on a tap, opens the notice's link. A push carries
 * `{"notice":"<32 lowercase hex>","text":"<key>"}` and nothing else (NT-1); any other payload is
 * dropped and nothing is shown. It has no fetch handler and keeps nothing: no cache, no storage, no
 * message log (P5). The link is `<this origin>/n/<notice>` (NT-4), built here, never taken from the
 * payload.
 */

const TEXTS = Object.freeze({
  approval_needed: "An agent in your workspace needs your approval",
  attention_needed: "Your workspace has a new alert",
  account_changed: "There was a change to your account or workspace access",
  brief_ready: "Your daily brief is ready",
});

const NOTICE = /^[0-9a-f]{32}$/;

/**
 * The notice in a push's text, or null for anything that is not exactly the closed pair in its
 * canonical bytes: no duplicate key, whitespace, byte order mark, or other order is accepted.
 */
function readNotice(raw) {
  if (typeof raw !== "string") return null;
  let value;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (value === null || typeof value !== "object") return null;
  const { notice, text } = value;
  if (typeof notice !== "string" || !NOTICE.test(notice)) return null;
  if (typeof text !== "string" || !Object.hasOwn(TEXTS, text)) return null;
  if (JSON.stringify({ notice, text }) !== raw) return null;
  return { notice, text };
}

function onPush(event) {
  const raw = event.data ? event.data.text() : "";
  const notice = readNotice(raw);
  if (notice === null) return;
  event.waitUntil(
    self.registration.showNotification("Owlhead", {
      body: TEXTS[notice.text],
      tag: notice.notice,
      data: { notice: notice.notice },
    }),
  );
}

function onNotificationClick(event) {
  event.notification.close();
  const notice = event.notification.data && event.notification.data.notice;
  if (typeof notice !== "string" || !NOTICE.test(notice)) return;
  event.waitUntil(self.clients.openWindow(new URL(`/n/${notice}`, self.location.origin).href));
}

self.addEventListener("push", onPush);
self.addEventListener("notificationclick", onNotificationClick);
