"use client";

import { type ReactNode, createContext, useContext, useEffect, useRef, useSyncExternalStore } from "react";
import Image from "next/image";
import { cn } from "@/lib/utils";
import { type Artwork, DAY, NIGHT, WALLPAPERS, artwork } from "./art";
import { useDesktopStyle, useSetDesktopStyle } from "./desktop-style";
import { BOLD, BUTTON, LINK, PIXEL, SUNKEN } from "./letter";
import styles from "./letter.module.css";
import { MAC, PC, PixelIcon } from "./pixel-icons";

export const AUTO = "auto";

/** The painting this visit opened on, which the server picks at random from the wallpapers (DEC-905). */
const VisitWallpaper = createContext<string>(AUTO);

export function WallpaperProvider({ initial, children }: { initial: string; children: ReactNode }) {
  return <VisitWallpaper value={initial}>{children}</VisitWallpaper>;
}

// The site keeps nothing in browser storage, so a picked wallpaper lasts until the page is reloaded.
let picked: string | null = null;
const listeners = new Set<() => void>();

function subscribe(onChange: () => void): () => void {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
}

const stored = () => picked;

function choose(id: string) {
  picked = id;
  for (const l of listeners) l();
}

/** The picked wallpaper, else the visit's: a painting, or "auto", one by day and another by night. */
function useWallpaper(): string {
  const visit = useContext(VisitWallpaper);
  return useSyncExternalStore(subscribe, stored, () => null) ?? visit;
}

function Picture({ art, className, sizes }: { art: Artwork; className?: string; sizes: string }) {
  return <Image src={art.src} alt="" fill sizes={sizes} loading="eager" className={cn("object-cover", className)} style={{ objectPosition: art.focus }} />;
}

/**
 * Leans the painting a few pixels away from the pointer, so the desktop has depth behind its icons.
 * Only a fine pointer moves it, and never under reduced motion.
 */
function useParallax() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || !window.matchMedia?.("(pointer: fine) and (prefers-reduced-motion: no-preference)").matches) return;
    let frame = 0;
    const lean = (e: PointerEvent) => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        el.style.setProperty("--lean-x", (e.clientX / window.innerWidth - 0.5).toFixed(3));
        el.style.setProperty("--lean-y", (e.clientY / window.innerHeight - 0.5).toFixed(3));
      });
    };
    window.addEventListener("pointermove", lean, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", lean);
    };
  }, []);
  return ref;
}

/**
 * The painting behind the desktop, drifting slowly as a museum film pans across a canvas (DEC-905).
 * Night dims it a little, the way a monitor turned down would.
 */
export function Wallpaper() {
  const choice = useWallpaper();
  const lean = useParallax();
  return (
    <div aria-hidden className="absolute inset-0 -z-10 overflow-hidden bg-muted" data-slot="wallpaper" data-wallpaper={choice}>
      <div ref={lean} className={cn(styles.lean, styles.bootWallpaper)}>
        <div className={cn(styles.drift, "absolute inset-0")}>
          {choice === AUTO ? (
            <>
              <Picture art={artwork(DAY)} sizes="100vw" className={styles.day} />
              <Picture art={artwork(NIGHT)} sizes="100vw" className={styles.night} />
            </>
          ) : (
            <Picture art={artwork(choice)} sizes="100vw" />
          )}
        </div>
      </div>
      <div className={cn(styles.dim, "absolute inset-0")} />
    </div>
  );
}

function Credit({ art }: { art: Artwork }) {
  return (
    <p className="text-[0.9375rem] leading-snug">
      {art.artist}, <i>{art.title}</i>, {art.date}. {art.medium}. The Metropolitan Museum of Art, public domain.{" "}
      <a href={art.url} target="_blank" rel="noreferrer" className={LINK}>
        See it at the Met
      </a>
    </p>
  );
}

/** The two desktops, as their computers: named for assistive technology only, since on screen neither says what it is (DEC-904, DEC-905). */
const COMPUTERS = [
  { style: "windows", name: "Windows 98", sprite: PC },
  { style: "mac", name: "System 7", sprite: MAC },
] as const;

/** Display Properties: pick the wallpaper from the Met's paintings and prints, shown on a small monitor, and the desktop. */
export function DisplayProperties({ onDone }: { onDone: () => void }) {
  const choice = useWallpaper();
  const style = useDesktopStyle();
  const setStyle = useSetDesktopStyle();
  const shown = artwork(choice === AUTO ? DAY : choice);
  return (
    <div className="grid gap-3 p-3 sm:p-4">
      <div aria-hidden className="grid justify-items-center">
        <div className={cn(styles.monitor, "p-2 pb-3")}>
          <div className={cn(SUNKEN, "relative h-28 w-44 overflow-hidden bg-muted")}>
            <Picture art={shown} sizes="176px" />
          </div>
        </div>
        <div className={cn(styles.monitor, "h-2 w-10")} />
        <div className={cn(styles.monitor, "h-1.5 w-24")} />
      </div>

      <fieldset className="grid gap-1">
        <legend className={cn(BOLD, "pb-1.5")}>Wallpaper</legend>
        <div className={cn(SUNKEN, "grid max-h-44 overflow-y-auto bg-card py-0.5")}>
          {[{ id: AUTO, title: "Day and night" }, ...WALLPAPERS].map((w) => (
            <label key={w.id} className={cn("flex cursor-pointer items-center gap-2 px-2 py-0.5 text-[0.9375rem] has-checked:bg-foreground has-checked:text-card", PIXEL)}>
              <input type="radio" name="wallpaper" value={w.id} checked={choice === w.id} onChange={() => choose(w.id)} className="accent-foreground" />
              {w.title}
            </label>
          ))}
        </div>
      </fieldset>

      {choice === AUTO ? (
        <p className="text-[0.9375rem] leading-snug">
          Hiroshige&apos;s <i>Autumn Moon at Ishiyama</i> by day and Dahl&apos;s <i>Copenhagen Harbor by Moonlight</i> at night, from The Metropolitan Museum of Art, public domain.
        </p>
      ) : (
        <Credit art={shown} />
      )}

      <fieldset className="flex items-center gap-3" data-slot="desktop-choice">
        <legend className={cn(BOLD, "float-left pe-1")}>Desktop</legend>
        {COMPUTERS.map((c) => (
          <label key={c.style} className={cn(SUNKEN, "grid size-12 cursor-pointer place-items-center bg-card has-checked:bg-highlight has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-foreground")}>
            <input type="radio" name="desktop" aria-label={c.name} checked={style === c.style} onChange={() => setStyle(c.style)} className="sr-only" />
            <PixelIcon sprite={c.sprite} />
          </label>
        ))}
      </fieldset>

      <p className="flex justify-end">
        <button type="button" onClick={onDone} className={BUTTON}>
          OK
        </button>
      </p>
    </div>
  );
}

export function ArtCredit({ art }: { art: Artwork }) {
  return <Credit art={art} />;
}
