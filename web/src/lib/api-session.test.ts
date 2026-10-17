import { describe, expect, it } from "vitest";
import { tokenFrom } from "./api-session";

describe("the access token traded for an API session", () => {
  it("is the sign-in client's token, none when signed out, and unreachable when the provider does not answer", async () => {
    expect(await tokenFrom(async () => ({ data: { session: { access_token: "eyJ.t" } }, error: null }))).toBe("eyJ.t");
    expect(await tokenFrom(async () => ({ data: { session: null }, error: null }))).toBeNull();
    expect(await tokenFrom(async () => ({ data: { session: null }, error: { status: 401, name: "AuthApiError" } }))).toBeNull();
    expect(await tokenFrom(async () => ({ data: { session: null }, error: { status: 503, name: "AuthRetryableFetchError" } }))).toBe("unreachable");
    expect(await tokenFrom(async () => ({ data: { session: null }, error: { name: "AuthRetryableFetchError" } }))).toBe("unreachable");
    expect(await tokenFrom(() => Promise.reject(new TypeError("offline")))).toBe("unreachable");
  });
});
