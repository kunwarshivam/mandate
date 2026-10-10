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

/** One of the app's screens as a picture, in light or dark as the page is. */
export function Shot({
  name,
  alt,
  sizes,
  className,
  frame = true,
}: {
  name: ShotName;
  alt: string;
  sizes: string;
  className?: string;
  frame?: boolean;
}) {
  const { width, height } = SHOTS[name];
  return (
    <div className={cn(frame && styles.shot, className)} data-slot="shot" data-shot={name}>
      <Image src={`/landing/${name}-light.png`} alt={alt} width={width} height={height} sizes={sizes} className={styles.light} />
      <Image src={`/landing/${name}-dark.png`} alt={alt} width={width} height={height} sizes={sizes} className={styles.dark} />
    </div>
  );
}
