"use client";

import { useState } from "react";
import { cn } from "@/lib/utils";
import { PIXEL, PLAIN_BUTTON, SUNKEN } from "./letter";
import { TOUR, TOUR_SECONDS, TOUR_VIDEO } from "./tour";

/** Media Player, open on Tour.mp4. Nothing of the video loads until someone presses play. */
export function MediaPlayer() {
  const [transcript, setTranscript] = useState(false);
  return (
    <div className={cn(PIXEL, "flex min-h-0 flex-1 flex-col gap-1.5 p-1.5")}>
      <div className={cn(SUNKEN, "bg-ink")}>
        <video controls playsInline preload="none" poster={TOUR_VIDEO.poster} className="aspect-video w-full bg-ink" data-slot="tour-video">
          <source src={TOUR_VIDEO.src} type="video/mp4" />
          <track kind="captions" src={TOUR_VIDEO.captions} srcLang="en" label="English" />
        </video>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2 px-0.5 text-[0.875rem]">
        <span>
          Tour.mp4 · {Math.floor(TOUR_SECONDS / 60)}:{String(TOUR_SECONDS % 60).padStart(2, "0")} · music: Scott Joplin, Maple Leaf Rag, 1916
        </span>
        <button type="button" aria-expanded={transcript} onClick={() => setTranscript((v) => !v)} className={PLAIN_BUTTON}>
          {transcript ? "Hide transcript" : "Transcript"}
        </button>
      </div>
      {transcript && (
        <ol className={cn(SUNKEN, "min-h-0 flex-1 overflow-y-auto bg-card px-3 py-2 text-[0.9375rem] leading-snug")} aria-label="Transcript">
          {TOUR.map((s) => (
            <li key={s.id} className="py-1">
              <span className="text-muted-foreground">0:{String(s.start).padStart(2, "0")}</span> {s.caption}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
