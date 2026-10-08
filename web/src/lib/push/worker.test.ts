import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * The shipped worker, `public/push-sw.js`, run against a fake `self`: what it shows for a push and
 * what it opens on a tap (E8-14 S8d; notifications spec §4.2, NT-1, NT-4, P5).
 */
const SOURCE = readFileSync(resolve(__dirname, "../../../public/push-sw.js"), "utf8");

const TEXTS: Record<string, string> = {
  approval_needed: "An agent in your workspace needs your approval",
  attention_needed: "Your workspace has a new alert",
  account_changed: "There was a change to your account or workspace access",
  brief_ready: "Your daily brief is ready",
};
const HEX = "0123456789abcdef0123456789abcdef";
const ORIGIN = "https://app.owlhead.invalid";

type Shown = { title: string; options: { body: string; tag: string; data: unknown } };

function load() {
  const listeners = new Map<string, (event: unknown) => void>();
  const shown: Shown[] = [];
  const opened: string[] = [];
  const waits: Promise<unknown>[] = [];
  const self = {
    location: { origin: ORIGIN },
    addEventListener: (type: string, listener: (event: unknown) => void) => listeners.set(type, listener),
    registration: {
      showNotification: (title: string, options: Shown["options"]) => {
        shown.push({ title, options });
        return Promise.resolve();
      },
    },
    clients: {
      openWindow: (url: string) => {
        opened.push(url);
        return Promise.resolve(null);
      },
    },
  };
  new Function("self", SOURCE)(self);
  const push = (raw: string | null) =>
    listeners.get("push")?.({
      data: raw === null ? null : { text: () => raw },
      waitUntil: (p: Promise<unknown>) => waits.push(p),
    });
  const click = (data: unknown) => {
    let closed = false;
    listeners.get("notificationclick")?.({
      notification: { data, close: () => (closed = true) },
      waitUntil: (p: Promise<unknown>) => waits.push(p),
    });
    return closed;
  };
  return { listeners, shown, opened, push, click };
}

describe("the push service worker", () => {
  it("listens only for push and notificationclick: no fetch handler, so it serves and caches nothing (P5)", () => {
    expect([...load().listeners.keys()].sort()).toEqual(["notificationclick", "push"]);
    expect(SOURCE).not.toMatch(/\bcaches\b|indexedDB|localStorage|sessionStorage|importScripts|fetch\(/);
  });

  it.skip("pending E8-14: shows the fixed sentence for each of the four text keys, and nothing of the payload but the notice as tag", () => {
    for (const [key, text] of Object.entries(TEXTS)) {
      const { shown, push } = load();
      push(JSON.stringify({ notice: HEX, text: key }));
      expect(shown).toEqual([{ title: "Owlhead", options: { body: text, tag: HEX, data: { notice: HEX } } }]);
    }
  });

  it.skip("pending E8-14: drops every payload that is not exactly the closed pair, and shows nothing (NT-1)", () => {
    const canary = "CANARY-AAPL-buy-100@187.25";
    const refused = [
      null,
      "",
      "not json",
      "null",
      "[]",
      JSON.stringify({ notice: HEX }),
      JSON.stringify({ text: "brief_ready" }),
      JSON.stringify({ notice: HEX, text: "brief_ready", body: canary }),
      JSON.stringify({ notice: HEX, text: canary }),
      JSON.stringify({ notice: HEX, text: "toString" }),
      JSON.stringify({ notice: HEX, text: "__proto__" }),
      JSON.stringify({ notice: HEX.toUpperCase(), text: "brief_ready" }),
      JSON.stringify({ notice: HEX.slice(1), text: "brief_ready" }),
      JSON.stringify({ notice: `${HEX}0`, text: "brief_ready" }),
      JSON.stringify({ notice: `../${HEX.slice(3)}`, text: "brief_ready" }),
      JSON.stringify({ notice: 7, text: "brief_ready" }),
      JSON.stringify({ notice: HEX, text: ["brief_ready"] }),
    ];
    for (const raw of refused) {
      const { shown, push } = load();
      push(raw);
      expect(shown, String(raw)).toEqual([]);
    }
  });

  it.skip("pending E8-14: shows only the four sentences whatever arrives (NT-1, fuzzed)", () => {
    const allowed = new Set(Object.values(TEXTS));
    const { shown, push } = load();
    let seed = 20261008;
    const next = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31);
    const pieces = ['{"notice":"', HEX, '","text":"', "approval_needed", "brief", '"}', ",", '"x":1', "\\u0000", "<script>"];
    const keys = Object.keys(TEXTS);
    for (let i = 0; i < 2000; i += 1) {
      const length = next() % 8;
      push(Array.from({ length }, () => pieces[next() % pieces.length]).join(""));
      if (i % 10 === 0) push(JSON.stringify({ notice: HEX, text: keys[next() % keys.length] }));
    }
    expect(shown.length).toBeGreaterThanOrEqual(200);
    for (const { title, options } of shown) {
      expect(title).toBe("Owlhead");
      expect(allowed.has(options.body)).toBe(true);
      expect(options.tag).toMatch(/^[0-9a-f]{32}$/);
    }
  });

  it.skip("pending E8-14: opens only <origin>/n/<notice> on a tap, and nothing for a notification without a valid notice (NT-4)", () => {
    const { opened, click } = load();
    expect(click({ notice: HEX })).toBe(true);
    expect(opened).toEqual([`${ORIGIN}/n/${HEX}`]);
    for (const data of [null, {}, { notice: "https://evil.example/" }, { notice: `${HEX}?t=1` }, { notice: 1 }]) {
      click(data);
    }
    expect(opened).toEqual([`${ORIGIN}/n/${HEX}`]);
  });
});
