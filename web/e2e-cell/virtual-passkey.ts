import { readFileSync, rmSync, writeFileSync } from "node:fs";
import type { CDPSession, Page } from "@playwright/test";

/**
 * A passkey for the cell's gestures, held by Chromium's virtual WebAuthn authenticator (the DevTools
 * protocol's `WebAuthn` domain): a platform authenticator with user verification, so the page's own
 * `navigator.credentials` calls make and use a real ES256 key pair, and the API verifies real
 * attestations and assertions. Test infrastructure only: nothing in `web/src` knows it is virtual.
 * Its key leaves the browser the run started only through `exportTo`, to the one file the driver
 * names under `target/cell` in a dev or ci cell, which the driver deletes at teardown and the leak
 * guard refuses as an artifact (DEC-2411), so a later run of the same cell can step up with it. It
 * is a virtual authenticator's test key, never a member's. Paper only, as every cell gesture is.
 */
export interface VirtualPasskey {
  /** How many credentials the authenticator holds, so a spec can tell an enrolment made one. */
  credentials(): Promise<number>;
  /** Removes the authenticator, so a later page has no passkey. */
  remove(): Promise<void>;
  /**
   * Writes every credential the authenticator holds to `path`, for a later run of this cell, and
   * answers how many: any file already there is deleted first and the new one created exclusively
   * at 0600, so the key never sits in a file another user can read, even for a moment.
   */
  exportTo(path: string): Promise<number>;
  /** Adds the credentials a run wrote with `exportTo` at `path`; answers how many. */
  importFrom(path: string): Promise<number>;
}

/** Attaches a virtual authenticator to `page` that answers every WebAuthn call at once, as a member touching their key would. */
export async function virtualPasskey(page: Page): Promise<VirtualPasskey> {
  const cdp: CDPSession = await page.context().newCDPSession(page);
  await cdp.send("WebAuthn.enable", { enableUI: false });
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  const held = () => cdp.send("WebAuthn.getCredentials", { authenticatorId });
  return {
    async credentials() {
      return (await held()).credentials.length;
    },
    async remove() {
      await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
    },
    async exportTo(path) {
      const { credentials } = await held();
      rmSync(path, { force: true });
      writeFileSync(path, JSON.stringify({ credentials }), { mode: 0o600, flag: "wx" });
      return credentials.length;
    },
    async importFrom(path) {
      const { credentials } = JSON.parse(readFileSync(path, "utf8")) as Awaited<ReturnType<typeof held>>;
      for (const credential of credentials) await cdp.send("WebAuthn.addCredential", { authenticatorId, credential });
      return credentials.length;
    },
  };
}
