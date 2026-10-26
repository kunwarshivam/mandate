"use client";

import { useRef } from "react";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { Close } from "pixelarticons/react/Close.js";
import { KEY } from "@/components/kumo/key";
import { cn } from "@/lib/utils";

/**
 * D10's confirmation of a kill switch at any scope (DEC-2410): one press to confirm, no passkey. A
 * kill switch is never refused and needs no step-up for its stop and flatten (identity spec ID-5,
 * workspace API API-7, mandate spec §6.1, `AGENTS.md` rule 13), so nothing here can hold it back
 * but the owner's own Cancel. Focus lands on the dialog itself, never on an action, as G3's does.
 */
export function ConfirmDialog({ action, open, onConfirm, onCancel }: { action: string; open: boolean; onConfirm: () => void; onCancel: () => void }) {
  const popupRef = useRef<HTMLDivElement>(null);
  const focusOnMount = (node: HTMLDivElement | null) => {
    popupRef.current = node;
    node?.focus();
  };
  return (
    <Dialog.Root open={open} onOpenChange={(next) => (next ? undefined : onCancel())}>
      <Dialog.Portal>
        <Dialog.Backdrop data-slot="sheet-backdrop" className="fixed inset-0 z-60 bg-ink/40" />
        <Dialog.Popup
          ref={focusOnMount}
          initialFocus={popupRef}
          data-slot="confirm-dialog"
          className="fixed top-1/2 left-1/2 z-60 grid w-[calc(100%-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 gap-5 rounded-3xl border border-border bg-card p-6 text-foreground shadow-2xl outline-none"
        >
          <div className="grid gap-1.5">
            <Dialog.Title className="pr-10 text-h1">Confirm the kill switch</Dialog.Title>
            <Dialog.Description className="text-sm text-muted-foreground">No passkey is needed: a kill switch is never held back for a check. Paper account; simulated funds.</Dialog.Description>
            <Dialog.Close
              aria-label="Close"
              className="absolute top-3 right-3 grid size-11 place-items-center rounded-lg text-muted-foreground outline-none hover:bg-background hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring"
            >
              <Close className="size-6" aria-hidden />
            </Dialog.Close>
          </div>
          <p className="rounded-xl bg-background px-4 py-3 font-medium text-foreground" data-slot="confirm-action">
            {action}
          </p>
          <div className="flex flex-col-reverse gap-2 sm:flex-row sm:items-center sm:justify-end">
            <button type="button" onClick={onCancel} className={cn("min-w-28 shrink-0 whitespace-nowrap", KEY)}>
              Cancel
            </button>
            <button type="button" onClick={onConfirm} className={cn("min-w-28 shrink-0 whitespace-nowrap", KEY)}>
              Activate
            </button>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
