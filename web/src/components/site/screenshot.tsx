import { getImageProps } from "next/image";
import { cn } from "@/lib/utils";
import { SHOTS, type ShotName, shotSrc } from "./shots";

/**
 * One app screenshot, light or dark with the system: a `<picture>` whose dark source answers
 * `prefers-color-scheme`, so a visitor downloads one of the two. The landing page is for signed-out
 * visitors, who have no saved theme; an owner who picked a theme in the app and comes back signed out
 * sees the system's. Width and height are always set, so nothing moves while an image loads.
 */
export function Screenshot({ name, sizes, eager = false, className }: { name: ShotName; sizes: string; eager?: boolean; className?: string }) {
  const { width, height, alt } = SHOTS[name];
  const common = { alt, width, height, sizes, quality: 90 } as const;
  const loading = eager ? { fetchPriority: "high", loading: "eager" } as const : { loading: "lazy" } as const;
  const light = getImageProps({ ...common, ...loading, src: shotSrc(name, "light") }).props;
  const dark = getImageProps({ ...common, src: shotSrc(name, "dark") }).props;
  return (
    <picture data-slot="screenshot" data-shot={name}>
      <source media="(prefers-color-scheme: dark)" srcSet={dark.srcSet} sizes={dark.sizes} width={width} height={height} />
      <img {...light} src={light.src} alt={alt} className={cn("block h-auto w-full", className)} />
    </picture>
  );
}
