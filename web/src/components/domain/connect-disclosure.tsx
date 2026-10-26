import { CONNECT_DISCLOSURE } from "@/api/step-up";

/**
 * The Alpaca OAuth token-breadth disclosure, document `alpaca_oauth_token_breadth` version `1`, in the
 * founder's wording exactly as DEC-2622 item 1 accepted it (2026-10-25). It is shown before the owner's
 * step-up, which binds this document and version (DEC-2621 item 4), so a changed wording is a new
 * version here and in `CONNECT_DISCLOSURE` together.
 */
export const DISCLOSURE_VERSION = CONNECT_DISCLOSURE.version;

export function ConnectDisclosure() {
  return (
    <section className="grid gap-3" aria-labelledby="connect-disclosure-title" data-slot="connect-disclosure" data-document={CONNECT_DISCLOSURE.document} data-version={CONNECT_DISCLOSURE.version}>
      <h2 id="connect-disclosure-title" className="text-h3">
        Before you connect Alpaca paper trading
      </h2>
      <p>Alpaca&rsquo;s documentation does not say whether the access it grants for your paper account can also be used with a live Alpaca account you hold.</p>
      <p>
        Mandate sends every order and every account request for this connection only to Alpaca&rsquo;s paper trading service, from a process that cannot reach Alpaca&rsquo;s live service. The one Mandate
        process that contacts Alpaca&rsquo;s live address does so only to exchange your sign-in code for access, and it cannot place orders.
      </p>
      <p>You can remove Mandate&rsquo;s access at any time in your Alpaca account&rsquo;s app settings. Revoking the connection in Mandate also deletes the access Mandate holds.</p>
    </section>
  );
}
