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
  ask: { width: 607, height: 506 },
  request: { width: 390, height: 844 },
  ladder: { width: 596, height: 556 },
} as const;

export type ShotName = keyof typeof SHOTS;

/** One of the app's screens, or a piece of one, as a picture in light or dark as the page is; `sizes` defaults to the width it was taken at. */
export function Shot({
  name,
  alt,
  sizes,
  className,
  frame = true,
}: {
  name: ShotName;
  alt: string;
  sizes?: string;
  className?: string;
  frame?: boolean;
}) {
  const { width, height } = SHOTS[name];
  const drawn = sizes ?? `(max-width: ${width}px) 100vw, ${width}px`;
  return (
    <div className={cn(frame && styles.shot, "w-full", className)} style={{ maxWidth: width }} data-slot="shot" data-shot={name}>
      <Image src={`/landing/${name}-light.png`} alt={alt} width={width} height={height} sizes={drawn} className={styles.light} />
      <Image src={`/landing/${name}-dark.png`} alt={alt} width={width} height={height} sizes={drawn} className={styles.dark} />
    </div>
  );
}
