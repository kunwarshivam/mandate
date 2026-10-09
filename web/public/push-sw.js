/*
 * Owlhead's push service worker (E8-14, slice S8d; notifications spec §4.2, §4.3, §4.6).
 *
 * Stub: E8-14's implementation shows one of four fixed sentences for a push that carries exactly a
 * notice id and a text key, and opens `<origin>/n/<notice>` on a tap. It has no fetch handler and
 * keeps nothing (P5).
 */

function onPush() {
  throw new Error("Unimplemented: E8-14");
}

function onNotificationClick() {
  throw new Error("Unimplemented: E8-14");
}

self.addEventListener("push", onPush);
self.addEventListener("notificationclick", onNotificationClick);
