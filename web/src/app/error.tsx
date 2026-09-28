"use client";

/**
 * A rendering failure changes nothing on the deployment: no request is sent from here. Reloading is
 * safe because this screen submits nothing.
 */
export default function ErrorScreen({ reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return (
    <section role="alert" className="grid max-w-2xl gap-3 rounded-xl border border-notice-border bg-notice p-5">
      <h1 className="text-heading">This screen failed to display</h1>
      <p>Nothing was sent or changed: displaying a screen never places or cancels an order. The Stop control in the header still works.</p>
      <p>
        <button type="button" onClick={reset} className="press rounded-lg border bg-card px-4 py-2 font-medium hover:bg-muted">
          Display it again
        </button>
      </p>
    </section>
  );
}
