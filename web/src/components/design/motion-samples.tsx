"use client";

import { type CSSProperties, type ReactNode, useState } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/primitives/dialog";
import { ModeBadge } from "@/components/domain/mode";
import { Money } from "@/components/domain/money";
import type { AgentMode } from "@/fixtures/types";

const MODES: AgentMode[] = ["normal", "exits_only", "paused", "stopped"];
const VALUES = ["10123.45", "10131.02", "10118.77"];

function Sample({ note, children, className }: { note: string; children: ReactNode; className?: string }) {
  return (
    <div className={`grid content-start gap-3 bg-card p-4 ${className ?? ""}`}>
      <p className="text-caption text-muted-foreground">{note}</p>
      {children}
    </div>
  );
}

export function MotionSamples() {
  const [mode, setMode] = useState(0);
  const [value, setValue] = useState(0);
  const [reveal, setReveal] = useState(0);
  return (
    <div className="grid gap-(--seam) sm:grid-cols-2">
      <Sample note="Press: scale 0.97 over 140 ms; release in 80 ms">
        <Button variant="outline" size="lg" className="press h-11 w-fit">
          Press and hold
        </Button>
      </Sample>
      <Sample note="Sheet: 280 ms on the drawer curve in, 200 ms out">
        <Dialog.Root>
          <Dialog.Trigger render={<Button variant="outline" size="lg" className="h-11 w-fit" />}>Open a sample sheet</Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Backdrop data-slot="sheet-backdrop" className="fixed inset-0 z-50 bg-ink/40" />
            <Dialog.Popup data-slot="stop-sheet" className="fixed inset-y-0 right-0 z-50 grid w-full content-start gap-2 border-l-2 border-foreground bg-card p-4 outline-none sm:max-w-md">
              <Dialog.Title className="text-heading">Sample sheet</Dialog.Title>
              <Dialog.Description className="text-muted-foreground">Enters on the drawer curve; leaves faster on ease-out.</Dialog.Description>
              <Dialog.Close render={<Button variant="secondary" className="h-11 w-fit" />}>Close</Dialog.Close>
            </Dialog.Popup>
          </Dialog.Portal>
        </Dialog.Root>
      </Sample>
      <Sample note="Mode change: the field changes colour over 160 ms and its width follows, 200 ms">
        <div className="flex items-center gap-3">
          <ModeBadge mode={MODES[mode]} />
          <Button variant="ghost" onClick={() => setMode((m) => (m + 1) % MODES.length)}>
            Next mode
          </Button>
        </div>
      </Sample>
      <Sample note="Number change: the whole value rolls up and out, 200 ms">
        <div className="flex items-center gap-3">
          <Money value={VALUES[value]} className="font-display text-3xl font-bold" />
          <Button variant="ghost" onClick={() => setValue((v) => (v + 1) % VALUES.length)}>
            New value
          </Button>
        </div>
      </Sample>
      <Sample note="Reveal: each field wipes in from its leading edge, 200 ms, 30 ms apart; once, on first render" className="sm:col-span-2">
        <ul key={reveal} className="grid grid-cols-3 gap-(--seam)">
          {["bg-muted", "bg-card ring-2 ring-foreground ring-inset", "bg-mandate"].map((c, i) => (
            <li key={c} className={`reveal h-14 ${c}`} style={{ "--i": i } as CSSProperties} />
          ))}
        </ul>
        <Button variant="ghost" className="w-fit" onClick={() => setReveal((r) => r + 1)}>
          Replay
        </Button>
      </Sample>
    </div>
  );
}
