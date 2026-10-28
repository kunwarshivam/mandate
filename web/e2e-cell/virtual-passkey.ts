import type { CDPSession, Page } from "@playwright/test";

/**
 * A passkey for the cell's gestures, held by Chromium's virtual WebAuthn authenticator (the DevTools
 * protocol's `WebAuthn` domain): a platform authenticator with user verification, so the page's own
 * `navigator.credentials` calls make and use a real ES256 key pair, and the API verifies real
 * attestations and assertions. Test infrastructure only: nothing in `web/src` knows it is virtual,
 * and the key never leaves the browser the run started. Paper only, as every cell gesture is.
 */
export interface VirtualPasskey {
  /** How many credentials the authenticator holds, so a spec can tell an enrolment made one. */
  credentials(): Promise<number>;
  /** Removes the authenticator, so a later page has no passkey. */
  remove(): Promise<void>;
}

/** Attaches a virtual authenticator to `page` that answers every WebAuthn call at once, as a member touching their key would. */
export async function virtualPasskey(page: Page): Promise<VirtualPasskey> {
  const cdp: CDPSession = await page.context().newCDPSession(page);
  await cdp.send("WebAuthn.enable", { enableUI: false });
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  return {
    async credentials() {
      const { credentials } = await cdp.send("WebAuthn.getCredentials", { authenticatorId });
      return credentials.length;
    },
    async remove() {
      await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
    },
  };
}
