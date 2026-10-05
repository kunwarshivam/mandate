"use client";

import { KEY } from "@/components/kumo/key";

/**
 * A rendering failure changes nothing on the deployment: no request is sent from here. Reloading is
 * safe because this screen submits nothing.
 */
export default function ErrorScreen({ reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return (
    <section role="alert" className="grid max-w-2xl gap-4 pt-6 sm:pt-12">
      <h1 className="text-h1 sm:text-h1">This screen failed to display</h1>
      <p>Nothing was sent or changed: displaying a screen never places or cancels an order. The Stop control still works.</p>
      <p>
        <button type="button" onClick={reset} className={KEY}>
          Display it again
        </button>
      </p>
    </section>
  );
}
