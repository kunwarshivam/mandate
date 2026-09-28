"use client";

import { useEffect, useRef, useState } from "react";
import { Fingerprint } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";

/**
 * G3: a mocked passkey check. The real one is a WebAuthn assertion bound to this one action; here
 * the "passkey" answers after a short wait. Cancelling means the action did not happen.
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
  const contentRef = useRef<HTMLDivElement>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

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
    <Dialog open={open} onOpenChange={(next) => (next ? undefined : cancel())}>
      <DialogContent
        ref={contentRef}
        tabIndex={-1}
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          contentRef.current?.focus();
        }}
        className="gap-5 p-5 sm:max-w-md"
        data-slot="step-up"
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-heading">
            <Fingerprint className="size-5 text-primary" aria-hidden />
            Confirm it is you
          </DialogTitle>
          <DialogDescription>Use your passkey to authorize this one action. Paper account; simulated funds.</DialogDescription>
        </DialogHeader>
        <p className="rounded-lg bg-muted p-3 text-sm font-medium text-foreground" data-slot="step-up-action">
          {action}
        </p>
        <p role="status" aria-live="polite" className="min-h-5 text-sm text-muted-foreground">
          {waiting ? "Waiting for your passkey…" : ""}
        </p>
        <DialogFooter className="-mx-5 -mb-5 p-5">
          <Button variant="outline" size="lg" className="press min-w-28" onClick={cancel}>
            Cancel
          </Button>
          <Button size="lg" className="press min-w-28" onClick={verify}>
            Use passkey
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
