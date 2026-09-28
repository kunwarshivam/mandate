"use client";

import { createContext, useContext, useEffect, useRef, useState } from "react";
import { Fingerprint } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";

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
 * still in flight reaches `onVerified`.
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
  const contentRef = useRef<HTMLDivElement>(null);
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
    <Dialog open={open} onOpenChange={(next) => (next ? undefined : cancel())}>
      <DialogContent
        ref={contentRef}
        tabIndex={-1}
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          contentRef.current?.focus();
        }}
        className="gap-4 p-4 sm:max-w-md"
        data-slot="step-up"
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 pr-10 text-title">
            <Fingerprint className="size-6 shrink-0 text-primary" aria-hidden />
            Confirm it is you
          </DialogTitle>
          <DialogDescription>Use your passkey to authorize this one action. Paper account; simulated funds.</DialogDescription>
        </DialogHeader>
        <p className="border-t-2 border-foreground bg-muted px-3 py-2.5 font-bold text-foreground" data-slot="step-up-action">
          {action}
        </p>
        <DialogFooter>
          <p role="status" aria-live="polite" className="text-sm text-muted-foreground sm:mr-auto sm:self-center">
            {waiting ? "Waiting for your passkey…" : ""}
          </p>
          <Button variant="outline" size="lg" className="min-w-28" onClick={cancel}>
            Cancel
          </Button>
          <Button size="lg" className="min-w-28" onClick={verify}>
            Use passkey
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
