import { describe, expect, it } from "vitest";
import { config } from "@/proxy";
import { isPublicPath } from "@/lib/auth-routes";

/** E8-14 S8d: the worker script is fetched with no session, so neither the proxy nor sign-in may redirect it. */
const matcher = new RegExp(`^${config.matcher[0]}$`);

describe("the push worker's route", () => {
  it("is outside the proxy, so a signed-out update check still reaches the script", () => {
    expect(matcher.test("/push-sw.js")).toBe(false);
    expect(matcher.test("/settings/notifications")).toBe(true);
    expect(matcher.test("/n/0123456789abcdef0123456789abcdef")).toBe(true);
  });

  it("is a public file, and nothing else under its name is", () => {
    expect(isPublicPath("/push-sw.js")).toBe(true);
    for (const path of ["/push-sw.js/x", "/push-sw", "/n/0123456789abcdef0123456789abcdef"]) {
      expect(isPublicPath(path), path).toBe(false);
    }
  });
});
