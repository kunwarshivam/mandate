"use client";

import { useSyncExternalStore } from "react";
import Image from "next/image";
import { cn } from "@/lib/utils";
import { type Artwork, DAY, NIGHT, WALLPAPERS, artwork } from "./art";
import { BOLD, BUTTON, LINK, PIXEL, SUNKEN } from "./letter";
import styles from "./letter.module.css";
import { SaverSettings } from "./screensaver";

const AUTO = "auto";

// The site keeps nothing in browser storage, so a picked wallpaper lasts until the page is reloaded.
let picked = AUTO;
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

/** The picked wallpaper, or "auto": a painting by day and another by night, following the theme. */
function useWallpaper(): string {
  return useSyncExternalStore(subscribe, stored, () => AUTO);
}

function Picture({ art, className, sizes }: { art: Artwork; className?: string; sizes: string }) {
  return <Image src={art.src} alt="" fill sizes={sizes} loading="eager" className={cn("object-cover", className)} style={{ objectPosition: art.focus }} />;
}

/** The painting behind the desktop. Night dims it a little, the way a monitor turned down would. */
export function Wallpaper() {
  const choice = useWallpaper();
  return (
    <div aria-hidden className="absolute inset-0 -z-10 bg-muted" data-slot="wallpaper" data-wallpaper={choice}>
      {choice === AUTO ? (
        <>
          <Picture art={artwork(DAY)} sizes="100vw" className={styles.day} />
          <Picture art={artwork(NIGHT)} sizes="100vw" className={styles.night} />
        </>
      ) : (
        <Picture art={artwork(choice)} sizes="100vw" />
      )}
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

/** Display Properties: pick the wallpaper from the Met's paintings and prints, shown on a small monitor, and the screen saver. */
export function DisplayProperties({ onDone, onPreview }: { onDone: () => void; onPreview: () => void }) {
  const choice = useWallpaper();
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
          Hokusai&apos;s <i>Great Wave</i> by day and Dahl&apos;s <i>Copenhagen Harbor by Moonlight</i> at night, from The Metropolitan Museum of Art, public domain.
        </p>
      ) : (
        <Credit art={shown} />
      )}

      <SaverSettings onPreview={onPreview} />

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
