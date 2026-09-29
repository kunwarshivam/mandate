import { describe, expect, it } from "vitest";
import { parseBetaRequest } from "./beta";

describe("a private beta request", () => {
  it("keeps a trimmed, lowercased address and a known role", () => {
    expect(parseBetaRequest({ email: "  Ada@Example.COM ", role: "desk" })).toEqual({ ok: true, request: { email: "ada@example.com", role: "desk" } });
  });

  it("drops a role it does not know", () => {
    expect(parseBetaRequest({ email: "ada@example.com", role: "admin" })).toEqual({ ok: true, request: { email: "ada@example.com", role: null } });
  });

  it.each([[undefined], [null], ["nope"], [{ email: "" }], [{ email: "ada@" }], [{ email: "a b@c.d" }], [{ email: `${"a".repeat(250)}@x.io` }]])("refuses %j", (body) => {
    expect(parseBetaRequest(body)).toEqual({ ok: false, reason: "email" });
  });

  it("marks a filled hidden field as a bot", () => {
    expect(parseBetaRequest({ email: "ada@example.com", website: "spam.biz" })).toEqual({ ok: false, reason: "bot" });
  });
});
