"use client";

import { useEffect, useRef, useSyncExternalStore } from "react";
import { cn } from "@/lib/utils";
import { BOLD, MONO, PLAIN_BUTTON, SUNKEN } from "./letter";

export type SaverKind = "owls" | "none";

type Saver = { kind: SaverKind; minutes: number };

export const WAITS = [1, 2, 5, 10] as const;

// Like the wallpaper, the choice lasts until the page is reloaded: the site keeps nothing in browser storage.
let saver: Saver = { kind: "owls", minutes: 1 };
const listeners = new Set<() => void>();

function subscribe(onChange: () => void): () => void {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
}

export function setSaver(patch: Partial<Saver>) {
  saver = { ...saver, ...patch };
  for (const l of listeners) l();
}

const DEFAULT: Saver = { kind: "owls", minutes: 1 };

export function useSaver(): Saver {
  return useSyncExternalStore(
    subscribe,
    () => saver,
    () => DEFAULT,
  );
}

const OWL = [
  "..kk....kk..",
  "..kbk..kbk..",
  "..kbbbbbbk..",
  ".kwwwbbwwwk.",
  ".kwkwbbwkwk.",
  ".kwwwyywwwk.",
  ".kbbbyybbbk.",
  "kbbtbbbbtbbk",
  "kbtbtbbtbtbk",
  "kbbtbbbbtbbk",
  ".kbbbbbbbbk.",
  "..kykkkkyk..",
];

const INKS: Record<string, string> = { k: "--ink-line", b: "--series-2", w: "--ink-foreground", y: "--highlight", t: "--series-5" };

type Flyer = { x: number; y: number; z: number; owl: boolean };

const spawn = (far = true): Flyer => ({ x: (Math.random() - 0.5) * 2, y: (Math.random() - 0.5) * 2, z: far ? 1 : 0.1 + Math.random() * 0.9, owl: Math.random() < 0.22 });

/** Flying Owls: owls and stars come out of the dark toward you, as Flying Windows did. */
function Sky({ still }: { still: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    const css = getComputedStyle(document.documentElement);
    const ink = Object.fromEntries(Object.entries(INKS).map(([k, v]) => [k, css.getPropertyValue(v).trim()]));
    const flyers = Array.from({ length: 140 }, () => spawn(false));
    let frame = 0;
    let last = performance.now();

    const draw = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      const w = Math.round(canvas.clientWidth * devicePixelRatio);
      const h = Math.round(canvas.clientHeight * devicePixelRatio);
      if (canvas.width !== w || canvas.height !== h) Object.assign(canvas, { width: w, height: h });
      ctx.fillStyle = css.getPropertyValue("--ink").trim();
      ctx.fillRect(0, 0, w, h);
      const f = Math.min(w, h) * 0.6;
      for (const p of flyers.toSorted((a, b) => b.z - a.z)) {
        if (!still) p.z -= dt * 0.22;
        const sx = w / 2 + (p.x / p.z) * f;
        const sy = h / 2 + (p.y / p.z) * f;
        if (p.z < 0.05 || sx < -80 || sy < -80 || sx > w + 80 || sy > h + 80) {
          Object.assign(p, spawn());
          continue;
        }
        if (!p.owl) {
          const s = Math.max(1, (1.2 - p.z) * 3 * devicePixelRatio);
          ctx.fillStyle = ink.w;
          ctx.fillRect(sx, sy, s, s);
          continue;
        }
        const px = Math.max(1, Math.round(((1 - p.z) * 7 + 0.6) * devicePixelRatio));
        OWL.forEach((row, y) => {
          for (let x = 0; x < row.length; x++) {
            const c = row[x];
            if (c === ".") continue;
            ctx.fillStyle = ink[c];
            ctx.fillRect(Math.round(sx + (x - 6) * px), Math.round(sy + (y - 6) * px), px, px);
          }
        });
      }
      if (!still) frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [still]);

  return <canvas ref={ref} aria-hidden className="size-full" />;
}

/** The screen saver, over everything. Any key, click or real move of the mouse wakes the desktop. */
export function ScreenSaver({ onWake }: { onWake: () => void }) {
  const still = useSyncExternalStore(
    (cb) => {
      const q = matchMedia("(prefers-reduced-motion: reduce)");
      q.addEventListener("change", cb);
      return () => q.removeEventListener("change", cb);
    },
    () => matchMedia("(prefers-reduced-motion: reduce)").matches,
    () => true,
  );

  useEffect(() => {
    const since = performance.now();
    let from: { x: number; y: number } | null = null;
    const wake = (e: Event) => {
      if (performance.now() - since < 400) return;
      if (e instanceof PointerEvent && e.type === "pointermove") {
        from ??= { x: e.clientX, y: e.clientY };
        if (Math.hypot(e.clientX - from.x, e.clientY - from.y) < 24) return;
      }
      onWake();
    };
    const events = ["pointermove", "pointerdown", "keydown", "wheel", "touchstart"] as const;
    for (const t of events) window.addEventListener(t, wake, { capture: true });
    return () => {
      for (const t of events) window.removeEventListener(t, wake, { capture: true });
    };
  }, [onWake]);

  return (
    <div role="presentation" className="fixed inset-0 z-[95] cursor-none bg-ink" data-slot="screen-saver">
      <Sky still={still} />
    </div>
  );
}

/** Wakes `onIdle` after the chosen wait with no key, click, scroll or move of the mouse. */
export function useIdle(minutes: number, enabled: boolean, onIdle: () => void) {
  useEffect(() => {
    if (!enabled || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    let timer = setTimeout(onIdle, minutes * 60_000);
    const reset = () => {
      clearTimeout(timer);
      timer = setTimeout(onIdle, minutes * 60_000);
    };
    const events = ["pointermove", "pointerdown", "keydown", "wheel", "touchstart", "scroll"] as const;
    for (const t of events) window.addEventListener(t, reset, { capture: true, passive: true });
    return () => {
      clearTimeout(timer);
      for (const t of events) window.removeEventListener(t, reset, { capture: true });
    };
  }, [minutes, enabled, onIdle]);
}

/** Display Properties' Screen saver panel. */
export function SaverSettings({ onPreview }: { onPreview: () => void }) {
  const { kind, minutes } = useSaver();
  const field = cn(SUNKEN, MONO, "h-8 bg-card px-1.5 text-[1.125rem] outline-none focus-visible:outline-1 focus-visible:outline-dotted focus-visible:outline-foreground");
  return (
    <fieldset className="grid gap-1.5">
      <legend className={cn(BOLD, "pb-1.5")}>Screen saver</legend>
      <div className="flex flex-wrap items-center gap-2">
        <select aria-label="Screen saver" value={kind} onChange={(e) => setSaver({ kind: e.target.value as SaverKind })} className={field}>
          <option value="owls">Flying Owls</option>
          <option value="none">(None)</option>
        </select>
        <button type="button" onClick={onPreview} disabled={kind === "none"} className={cn(PLAIN_BUTTON, "disabled:cursor-default disabled:opacity-50")}>
          Preview
        </button>
        <label className="flex items-center gap-1.5 text-[0.9375rem]">
          Wait
          <select aria-label="Wait" value={minutes} disabled={kind === "none"} onChange={(e) => setSaver({ minutes: Number(e.target.value) })} className={field}>
            {WAITS.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          minutes
        </label>
      </div>
    </fieldset>
  );
}
