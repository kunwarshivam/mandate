"use client";

import { useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import { ModeBadge } from "@/components/domain/mode";
import { Money } from "@/components/domain/money";
import type { AgentMode } from "@/fixtures/types";

const MODES: AgentMode[] = ["normal", "exits_only", "paused", "stopped"];
const VALUES = ["10123.45", "10131.02", "10118.77"];

export function MotionSamples() {
  const [mode, setMode] = useState(0);
  const [value, setValue] = useState(0);
  const [reveal, setReveal] = useState(0);
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <div className="grid gap-3 rounded-xl border bg-card p-4">
        <p className="text-caption text-muted-foreground">Press: scale 0.98, 150 ms, ease-out-quart</p>
        <Button variant="outline" size="lg" className="press w-fit">
          Press and hold
        </Button>
      </div>
      <div className="grid gap-3 rounded-xl border bg-card p-4">
        <p className="text-caption text-muted-foreground">Spring sheet: 400 ms linear() from spring(380, 36)</p>
        <Sheet>
          <SheetTrigger asChild>
            <Button variant="outline" size="lg" className="press w-fit">
              Open a sample sheet
            </Button>
          </SheetTrigger>
          <SheetContent className="sm:max-w-md">
            <SheetHeader>
              <SheetTitle>Sample sheet</SheetTitle>
              <SheetDescription>Enters on the spring token; leaves faster on ease-out-quart.</SheetDescription>
            </SheetHeader>
          </SheetContent>
        </Sheet>
      </div>
      <div className="grid gap-3 rounded-xl border bg-card p-4">
        <p className="text-caption text-muted-foreground">Layout animation on mode change, 240 ms</p>
        <div className="flex items-center gap-3">
          <ModeBadge mode={MODES[mode]} />
          <Button variant="ghost" size="sm" className="press" onClick={() => setMode((m) => (m + 1) % MODES.length)}>
            Next mode
          </Button>
        </div>
      </div>
      <div className="grid gap-3 rounded-xl border bg-card p-4">
        <p className="text-caption text-muted-foreground">Number change: the whole value swaps with a 2 px cross-blur, 200 ms</p>
        <div className="flex items-center gap-3">
          <Money value={VALUES[value]} className="text-xl font-semibold" />
          <Button variant="ghost" size="sm" className="press" onClick={() => setValue((v) => (v + 1) % VALUES.length)}>
            New value
          </Button>
        </div>
      </div>
      <div className="grid gap-3 rounded-xl border bg-card p-4 sm:col-span-2">
        <p className="text-caption text-muted-foreground">Staggered reveal: 240 ms each, 40 ms apart; used once, on the dashboard&apos;s agent cards</p>
        <AnimatePresence mode="wait">
          <motion.ul key={reveal} className="grid grid-cols-3 gap-2" initial="hidden" animate="shown" variants={{ hidden: {}, shown: { transition: { staggerChildren: 0.04 } } }}>
            {[0, 1, 2].map((i) => (
              <motion.li
                key={i}
                variants={{ hidden: { opacity: 0, y: 6 }, shown: { opacity: 1, y: 0, transition: { duration: 0.24, ease: [0.25, 1, 0.5, 1] } } }}
                className="h-14 rounded-lg bg-muted"
              />
            ))}
          </motion.ul>
        </AnimatePresence>
        <Button variant="ghost" size="sm" className="press w-fit" onClick={() => setReveal((r) => r + 1)}>
          Replay
        </Button>
      </div>
    </div>
  );
}
