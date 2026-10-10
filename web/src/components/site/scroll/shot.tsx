import Image from "next/image";
import { cn } from "@/lib/utils";
import styles from "./scroll.module.css";

/** The pictures `scripts/landing-shots.mjs` takes of the example app, at the CSS size it takes them. */
export const SHOTS = {
  agent: { width: 1280, height: 800 },
  thread: { width: 1280, height: 800 },
  gate: { width: 1024, height: 720 },
  request: { width: 390, height: 844 },
  mandate: { width: 1280, height: 600 },
} as const;

export type ShotName = keyof typeof SHOTS;

/**
 * Where on an element's top edge the flying owl lands, from 0 at the left to 1 at the right, which
 * way it turns its head, and whether it wears tide feathers there, where its sun ones would vanish.
 */
export interface PerchSpot {
  at: number;
  yaw: number;
  coat?: "sun" | "tide";
}

/** The data attributes `owl-flight.tsx` reads to land on an element. */
export function perchProps(spot?: PerchSpot) {
  return spot ? { "data-perch": "", "data-perch-at": String(spot.at), "data-perch-yaw": String(spot.yaw), "data-perch-coat": spot.coat ?? "sun" } : {};
}

/** One of the app's screens as a picture, in light or dark as the page is. */
export function Shot({
  name,
  alt,
  sizes,
  className,
  frame = true,
  perch,
}: {
  name: ShotName;
  alt: string;
  sizes: string;
  className?: string;
  frame?: boolean;
  perch?: PerchSpot;
}) {
  const { width, height } = SHOTS[name];
  return (
    <div className={cn(frame && styles.shot, className)} data-slot="shot" data-shot={name} {...perchProps(perch)}>
      <Image src={`/landing/${name}-light.png`} alt={alt} width={width} height={height} sizes={sizes} className={styles.light} />
      <Image src={`/landing/${name}-dark.png`} alt={alt} width={width} height={height} sizes={sizes} className={styles.dark} />
    </div>
  );
}
