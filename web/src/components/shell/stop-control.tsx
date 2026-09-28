"use client";

import { useState } from "react";
import { usePathname } from "next/navigation";
import { Octagon } from "lucide-react";
import { StopSheet } from "@/components/stop/stop-sheet";

/**
 * The Stop control is always rendered and never disabled. It does not wait for the dashboard or any
 * model to load (brief §5, rule 13). Ink, because ink is what a stopped agent looks like.
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
        className="press inline-flex h-11 items-center gap-2 bg-ink px-4 font-bold text-ink-foreground outline-none hover:bg-ink/85 focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2"
      >
        <Octagon className="size-4.5" aria-hidden />
        Stop
      </button>
      <StopSheet open={open} onOpenChange={setOpen} agentId={match ? match[1] : null} />
    </>
  );
}
