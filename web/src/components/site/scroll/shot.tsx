import Image from "next/image";
import { cn } from "@/lib/utils";
import styles from "./scroll.module.css";

/**
 * The pictures `scripts/landing-shots.mjs` takes of the example app, at the CSS size it takes them:
 * the agent's whole screen and the phone's, and otherwise only the piece that makes its part's point,
 * so each is drawn no larger than it was taken and its words read at the app's own size.
 */
export const SHOTS = {
  agent: { width: 1280, height: 800 },
  verdict: { width: 558, height: 95 },
  check: { width: 558, height: 147 },
  ask: { width: 607, height: 506 },
  request: { width: 390, height: 844 },
  ladder: { width: 596, height: 556 },
} as const;

export type ShotName = keyof typeof SHOTS;

/**
 * Where on an element's top edge the flying owl lands, from 0 at the left to 1 at the right, which
 * way it turns, and whether it goes pale there, on tide, where its ink would sink in.
 */
export interface PerchSpot {
  at: number;
  yaw: number;
  coat?: "ink" | "pale";
}

/** The data attributes `owl-flight.tsx` reads to land on an element. */
export function perchProps(spot?: PerchSpot) {
  return spot ? { "data-perch": "", "data-perch-at": String(spot.at), "data-perch-yaw": String(spot.yaw), "data-perch-coat": spot.coat ?? "ink" } : {};
}

/** One of the app's screens, or a piece of one, as a picture in light or dark as the page is; `sizes` defaults to the width it was taken at. */
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
  sizes?: string;
  className?: string;
  frame?: boolean;
  perch?: PerchSpot;
}) {
  const { width, height } = SHOTS[name];
  const drawn = sizes ?? `(max-width: ${width}px) 100vw, ${width}px`;
  return (
    <div className={cn(frame && styles.shot, "w-full", className)} style={{ maxWidth: width }} data-slot="shot" data-shot={name} {...perchProps(perch)}>
      <Image src={`/landing/${name}-light.png`} alt={alt} width={width} height={height} sizes={drawn} className={styles.light} />
      <Image src={`/landing/${name}-dark.png`} alt={alt} width={width} height={height} sizes={drawn} className={styles.dark} />
    </div>
  );
}
