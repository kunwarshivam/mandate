"use client";

import { useState } from "react";
import { Owl } from "@/components/domain/owl";
import { cn } from "@/lib/utils";
import { PIXEL, PLAIN_BUTTON } from "./letter";

/** Each tip is something the home page says, put the way a paperclip would. */
export const TIPS = [
  "Your rules are plain English. Owlhead shows you what it understood, in dollars, before you confirm them.",
  "Orders above a size you choose wait on your phone until you approve them.",
  "One Stop button halts every agent and cancels their open orders.",
  "Owlhead writes every decision down before it acts. Open The record and try editing a line.",
  "In the beta, agents trade on paper, with simulated money.",
  "As losses reach levels you set, it trades smaller, then only sells, then closes out and pauses.",
  "It can't take money out of your account, and it can't change its own rules.",
  "Right-click the desktop for more. Try Minesweeper, or the Recycle Bin.",
];

export const GREETING = "It looks like you're trying to let an AI trade for you. Would you like help?";

/** The owl in the corner, after the paperclip of its day. It says hello once a visit, after a pause. */
export function Assistant({ shown, onClose, onTour }: { shown: boolean; onClose: () => void; onTour: () => void }) {
  const [tip, setTip] = useState<number | null>(null);

  if (!shown) return null;

  const close = () => {
    setTip(null);
    onClose();
  };
  const tour = () => {
    setTip(null);
    onTour();
  };

  return (
    <aside aria-label="Owl assistant" className={cn(PIXEL, "absolute right-3 bottom-3 z-[65] flex w-[min(19rem,calc(100%-1.5rem))] flex-col items-end gap-1.5")} data-slot="assistant">
      <div className="relative w-full bg-card p-3 text-[0.9375rem] leading-snug shadow-[3px_3px_0_0_var(--foreground)] ring-1 ring-foreground">
        <button type="button" aria-label="Close the owl assistant" onClick={close} className="absolute top-1 right-1.5 grid size-5 cursor-pointer place-items-center leading-none outline-none hover:bg-foreground hover:text-card focus-visible:outline-1 focus-visible:outline-dotted focus-visible:outline-foreground">
          ×
        </button>
        <p role="status" className="pe-4">
          {tip === null ? GREETING : TIPS[tip]}
        </p>
        <div className="flex flex-wrap gap-1.5 pt-2.5">
          {tip === null ? (
            <>
              <button type="button" onClick={tour} className={PLAIN_BUTTON}>
                Show me the tour
              </button>
              <button type="button" onClick={() => setTip(0)} className={PLAIN_BUTTON}>
                Give me a tip
              </button>
              <button type="button" onClick={close} className={PLAIN_BUTTON}>
                Just look around
              </button>
            </>
          ) : (
            <>
              <button type="button" onClick={() => setTip((tip + 1) % TIPS.length)} className={PLAIN_BUTTON}>
                Next tip
              </button>
              <button type="button" onClick={tour} className={PLAIN_BUTTON}>
                Watch the tour
              </button>
            </>
          )}
        </div>
        <span aria-hidden className="absolute -bottom-2 right-10 size-3.5 rotate-45 border-r border-b border-foreground bg-card" />
      </div>
      <span className="me-5 motion-safe:animate-bounce [animation-duration:2.4s]">
        <Owl seed="assistant" mood="awake" className="size-14" />
      </span>
    </aside>
  );
}
