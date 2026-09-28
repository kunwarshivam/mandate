"use client";

import { useState } from "react";
import { usePathname } from "next/navigation";
import { Octagon } from "lucide-react";
import { StopSheet } from "@/components/stop/stop-sheet";

/**
 * The Stop control is always rendered and never disabled. It does not wait for the dashboard or any
 * model to load (brief §5, rule 13).
 */
export function StopControl() {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  const match = /^\/agents\/(agt_[0-9A-Z]+)/.exec(pathname);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        data-slot="stop-control"
        aria-haspopup="dialog"
        className="press inline-flex h-9 items-center gap-2 rounded-lg bg-ink px-3.5 text-sm font-semibold text-ink-foreground shadow-whisper outline-none hover:bg-ink/90 focus-visible:ring-3 focus-visible:ring-ring/60"
      >
        <Octagon className="size-4" aria-hidden />
        Stop
      </button>
      <StopSheet open={open} onOpenChange={setOpen} agentId={match ? match[1] : null} />
    </>
  );
}
