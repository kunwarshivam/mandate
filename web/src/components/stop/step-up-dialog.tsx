"use client";

import { createContext, useContext, useEffect, useRef, useState } from "react";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { Fingerprint, X } from "@phosphor-icons/react";

export type PasskeyResult = "verified" | "failed";

/**
 * One passkey request bound to one action. It calls `answer` at most once and returns a function
 * that abandons the request; after that, `answer` is never called.
 */
export type Passkey = (action: string, answer: (result: PasskeyResult) => void) => () => void;

export const PASSKEY_ANSWER_MS = 900;

/** The real check is a WebAuthn assertion; this one verifies after a short wait. */
export const mockPasskey: Passkey = (_action, answer) => {
  const id = window.setTimeout(() => answer("verified"), PASSKEY_ANSWER_MS);
  return () => window.clearTimeout(id);
};

export const PasskeyContext = createContext<Passkey>(mockPasskey);

/**
 * G3. Cancelled or failed means the action did not happen: only a verified answer to the request
 * still in flight reaches `onVerified`. Built on the Base UI primitive because Kumo's Dialog cannot
 * aim initial focus at the popup, and focus must land on the dialog itself, never on an action.
 */
export function StepUpDialog({
  action,
  open,
  onVerified,
  onCancel,
  onFailed,
}: {
  action: string;
  open: boolean;
  onVerified: () => void;
  onCancel: () => void;
  onFailed: () => void;
}) {
  const passkey = useContext(PasskeyContext);
  const [waiting, setWaiting] = useState(false);
  const popupRef = useRef<HTMLDivElement>(null);
  const abandon = useRef<(() => void) | null>(null);

  const [shownOpen, setShownOpen] = useState(open);
  if (open !== shownOpen) {
    setShownOpen(open);
    if (!open) setWaiting(false);
  }

  useEffect(() => () => abandon.current?.(), []);

  useEffect(() => {
    if (open) return;
    abandon.current?.();
    abandon.current = null;
  }, [open]);

  const focusOnMount = (node: HTMLDivElement | null) => {
    popupRef.current = node;
    node?.focus();
  };

  const cancel = () => {
    abandon.current?.();
    abandon.current = null;
    setWaiting(false);
    onCancel();
  };

  const verify = () => {
    if (abandon.current) return;
    let live = true;
    const release = passkey(action, (result) => {
      if (!live) return;
      live = false;
      abandon.current = null;
      setWaiting(false);
      if (result === "verified") onVerified();
      else onFailed();
    });
    if (!live) return;
    setWaiting(true);
    abandon.current = () => {
      live = false;
      release();
    };
  };

  return (
    <Dialog.Root open={open} onOpenChange={(next) => (next ? undefined : cancel())}>
      <Dialog.Portal>
        <Dialog.Backdrop data-slot="sheet-backdrop" className="fixed inset-0 z-60 bg-ink/40" />
        <Dialog.Popup
          ref={focusOnMount}
          initialFocus={popupRef}
          data-slot="step-up-dialog"
          className="fixed top-1/2 left-1/2 z-60 grid w-[calc(100%-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 gap-5 rounded-3xl border border-border bg-card p-6 text-foreground shadow-2xl outline-none"
        >
          <div className="grid gap-1.5">
            <Dialog.Title className="flex items-center gap-2 pr-10 text-h1">
              <Fingerprint className="size-6 shrink-0 text-lapis" aria-hidden />
              Confirm it is you
            </Dialog.Title>
            <Dialog.Description className="text-sm text-muted-foreground">
              Use your passkey to authorize this one action. Paper account; simulated funds.
            </Dialog.Description>
            <Dialog.Close
              aria-label="Close"
              className="absolute top-3 right-3 grid size-11 place-items-center rounded-lg text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
            >
              <X className="size-5" aria-hidden />
            </Dialog.Close>
          </div>
          <p className="rounded-xl bg-background px-4 py-3 font-medium text-foreground" data-slot="step-up-action">
            {action}
          </p>
          <div className="flex flex-col-reverse gap-2 sm:flex-row sm:items-center sm:justify-end">
            <p role="status" aria-live="polite" className="text-sm text-muted-foreground sm:mr-auto">
              {waiting ? "Waiting for your passkey…" : ""}
            </p>
            <button
              type="button"
              onClick={cancel}
              className="press h-11 min-w-28 rounded-lg border border-foreground/25 bg-card px-5 font-semibold outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={verify}
              className="press h-11 min-w-28 rounded-lg bg-lapis px-5 font-semibold text-lapis-foreground outline-none hover:bg-lapis-strong focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              Use passkey
            </button>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
