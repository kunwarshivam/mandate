"use client";

import { type CSSProperties, useState } from "react";
import Image from "next/image";
import { cn } from "@/lib/utils";
import { PIXEL, SUNKEN } from "./letter";
import styles from "./letter.module.css";

type Screen = {
  id: string;
  title: string;
  path: string;
  caption: string;
  /** Where the cursor points on the screen, in percent of its width and height. */
  at: { x: number; y: number };
};

/**
 * The app's own screens, captured from the example workspace by `scripts/app-screens.mjs` into
 * `public/app/<id>-<light|dark>.jpg`, in the order an owner meets them.
 */
export const APP_SCREENS: Screen[] = [
  {
    id: "home",
    title: "Home",
    path: "/",
    caption: "Home puts your money first, then what needs you. Here three agents are waiting for your answer before they buy.",
    at: { x: 79, y: 21 },
  },
  {
    id: "request",
    title: "A request",
    path: "/approvals/apr_01JBM9S346Q3D25VT4F5V37E3S",
    caption: "A request says what the agent wants to do, which of your rules sent it to you, and when it is skipped if you do nothing.",
    at: { x: 46, y: 61 },
  },
  {
    id: "agent",
    title: "An agent",
    path: "/agents/agt_01JB3K8Y4N7QW2M6R9T5V0XZAC",
    caption: "Each agent shows its money against its mandate, and how much room is left before each limit.",
    at: { x: 76, y: 36 },
  },
  {
    id: "record",
    title: "The record",
    path: "/agents/agt_01JB3K8Y4N7QW2M6R9T5V0XZAC/decisions",
    caption: "The record keeps every order an agent wanted, whether its rules allowed it, and why.",
    at: { x: 17, y: 41 },
  },
  {
    id: "mandate",
    title: "The mandate",
    path: "/agents/agt_01JB3K8Y4N7QW2M6R9T5V0XZAC/mandate",
    caption: "The mandate is the rules you set: the limits, and the levels where the agent slows down, sells or stops.",
    at: { x: 23, y: 61 },
  },
];

export const APP_ORIGIN = "https://app.owlhead.ai";

/** A pointer of the time, drawn in pixels. */
function Pointer() {
  return (
    <svg aria-hidden viewBox="0 0 12 18" shapeRendering="crispEdges" className="h-[18px] w-3 overflow-visible">
      <path d="M0 0V15L4 11L7 17L9 16L6 10H11Z" className="fill-foreground stroke-card" strokeWidth="1" strokeLinejoin="miter" />
    </svg>
  );
}

/**
 * The browser's second tab: the app itself, as it draws the example workspace, one screen at a time
 * (DEC-905). The screens play in turn, a cursor gliding to the part each one is about, until the
 * visitor picks one; pointing at the screen or focusing in it holds the one on show. Each turn is
 * timed by its progress bar's animation, so under reduced motion nothing plays and the visitor steps
 * through the screens.
 */
export function AppTab({ at, onAt }: { at: number; onAt: (i: number) => void }) {
  const [playing, setPlaying] = useState(true);
  const [held, setHeld] = useState(false);
  const screen = APP_SCREENS[at];
  const point = { "--x": `${screen.at.x}%`, "--y": `${screen.at.y}%` } as CSSProperties;

  return (
    <div
      className="grid gap-4 px-3 py-4 sm:px-6 sm:py-6"
      data-slot="app-tab"
      onPointerEnter={() => setHeld(true)}
      onPointerLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={() => setHeld(false)}
    >
      <ul aria-label="Screens" className={cn("-mx-3 flex gap-1.5 overflow-x-auto px-3 sm:mx-0 sm:grid sm:grid-cols-5 sm:overflow-visible sm:px-0", PIXEL)}>
        {APP_SCREENS.map((s, i) => {
          const on = i === at;
          return (
            <li key={s.id} className="grid min-w-[7.5rem] shrink-0 sm:min-w-0">
              <button
                type="button"
                aria-pressed={on}
                onClick={() => {
                  setPlaying(false);
                  onAt(i);
                }}
                className={cn(
                  "relative grid cursor-pointer gap-1.5 px-2 pt-1.5 pb-2 text-start text-[0.9375rem] leading-tight outline-none focus-visible:outline-2 focus-visible:outline-dotted focus-visible:outline-offset-2 focus-visible:outline-foreground",
                  on ? "text-foreground" : "text-foreground/60 hover:text-foreground",
                )}
              >
                {s.title}
                <span aria-hidden className="block h-1 overflow-hidden bg-foreground/15">
                  <span
                    key={on && playing ? `${s.id}-playing` : s.id}
                    className={cn("block h-full origin-left bg-foreground", on ? (playing ? styles.turn : "scale-x-100") : i < at ? "scale-x-100" : "scale-x-0")}
                    style={{ animationPlayState: held ? "paused" : "running" }}
                    onAnimationEnd={() => onAt((at + 1) % APP_SCREENS.length)}
                  />
                </span>
              </button>
            </li>
          );
        })}
      </ul>

      <figure className="grid gap-3">
        <div className={cn(SUNKEN, "relative aspect-[16/10] overflow-hidden bg-card")} data-slot="app-stage">
          {APP_SCREENS.map((s, i) => (
            <div key={s.id} aria-hidden={i !== at} className={cn(styles.shot, "absolute inset-0")} data-on={i === at || undefined}>
              <Image src={`/app/${s.id}-light.jpg`} alt={i === at ? `${s.title}, in the Owlhead app` : ""} fill sizes="(min-width: 64rem) 56rem, 100vw" className={cn(styles.day, "object-cover object-top")} />
              <Image src={`/app/${s.id}-dark.jpg`} alt="" fill sizes="(min-width: 64rem) 56rem, 100vw" className={cn(styles.night, "object-cover object-top")} />
            </div>
          ))}
          <div aria-hidden className={styles.cursor} style={point} data-slot="app-cursor">
            <span key={screen.id} className={styles.tap} />
            <Pointer />
            <span className={cn("ms-2.5 -mt-0.5 bg-highlight px-1.5 text-[0.8125rem] leading-snug text-highlight-foreground", PIXEL)}>You</span>
          </div>
        </div>
        <figcaption className="max-w-[44rem] text-pretty">
          {screen.caption}
        </figcaption>
      </figure>

      <p className="text-[0.9375rem] text-foreground/70">The example workspace, on paper money. These are the app&apos;s own screens, not drawings of it.</p>
    </div>
  );
}
