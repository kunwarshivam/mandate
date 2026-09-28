"use client";

import { useEffect, useRef, useState } from "react";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { Fingerprint } from "@phosphor-icons/react";

/**
 * G3: a mocked passkey check. The real one is a WebAuthn assertion bound to this one action; here
 * the "passkey" answers after a short wait. Cancelling means the action did not happen. Built on the
 * Base UI primitive because Kumo's Dialog cannot aim initial focus at the popup, and focus must land
 * on the dialog itself, never on an action.
 */
export function StepUpDialog({
  action,
  open,
  onVerified,
  onCancel,
  verifyAfterMs = 900,
}: {
  action: string;
  open: boolean;
  onVerified: () => void;
  onCancel: () => void;
  verifyAfterMs?: number;
}) {
  const [waiting, setWaiting] = useState(false);
  const popupRef = useRef<HTMLDivElement>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const focusOnMount = (node: HTMLDivElement | null) => {
    popupRef.current = node;
    node?.focus();
  };

  const cancel = () => {
    window.clearTimeout(timer.current);
    setWaiting(false);
    onCancel();
  };

  const verify = () => {
    setWaiting(true);
    timer.current = window.setTimeout(() => {
      setWaiting(false);
      onVerified();
    }, verifyAfterMs);
  };

  return (
    <Dialog.Root open={open} onOpenChange={(next) => (next ? undefined : cancel())}>
      <Dialog.Portal>
        <Dialog.Backdrop data-slot="sheet-backdrop" className="fixed inset-0 z-[60] bg-ink/40" />
        <Dialog.Popup
          ref={focusOnMount}
          initialFocus={popupRef}
          data-slot="step-up-dialog"
          className="fixed top-1/2 left-1/2 z-[60] grid w-[calc(100%-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 gap-4 border-2 border-foreground bg-card p-4 text-foreground outline-none"
        >
          <div className="grid gap-1.5">
            <Dialog.Title className="flex items-center gap-2 text-title">
              <Fingerprint className="size-6 shrink-0 text-lapis" aria-hidden />
              Confirm it is you
            </Dialog.Title>
            <Dialog.Description className="text-sm text-muted-foreground">
              Use your passkey to authorize this one action. Paper account; simulated funds.
            </Dialog.Description>
          </div>
          <p className="border-t-2 border-foreground bg-muted px-3 py-2.5 font-bold text-foreground" data-slot="step-up-action">
            {action}
          </p>
          <div className="flex flex-col-reverse gap-2 sm:flex-row sm:items-center sm:justify-end">
            <p role="status" aria-live="polite" className="text-sm text-muted-foreground sm:mr-auto">
              {waiting ? "Waiting for your passkey…" : ""}
            </p>
            <button
              type="button"
              onClick={cancel}
              className="press h-11 min-w-28 border-2 border-foreground bg-card px-4 font-bold outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={verify}
              className="press h-11 min-w-28 bg-lapis px-4 font-bold text-lapis-foreground outline-none hover:bg-lapis/90 focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              Use passkey
            </button>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
