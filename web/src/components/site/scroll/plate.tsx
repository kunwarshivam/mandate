import type { ReactNode } from "react";
import Image from "next/image";
import { cn } from "@/lib/utils";
import type { Artwork } from "../art";
import styles from "./scroll.module.css";

/**
 * A part of the long page set on one of the desktop's paintings, full bleed, credited in its corner
 * as every picture from the Met is where it is shown.
 */
export function Plate({ id, art, children, className }: { id: string; art: Artwork; children: ReactNode; className?: string }) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className="relative isolate scroll-mt-14 overflow-hidden bg-muted" data-slot="plate" data-art={art.id}>
      <div aria-hidden className="absolute inset-0 -z-10">
        <div className={cn(styles.plate, "absolute inset-x-0 -inset-y-[7%]")}>
          <Image src={art.src} alt="" fill sizes="100vw" className="object-cover" style={{ objectPosition: art.focus }} />
        </div>
      </div>
      <div className={className}>{children}</div>
      <p className="absolute right-3 bottom-3 max-w-[calc(100%-1.5rem)] rounded-full bg-card px-3 py-1 text-[0.8125rem] leading-snug text-foreground">
        <a href={art.url} target="_blank" rel="noreferrer" className="underline decoration-1 underline-offset-2 outline-none hover:decoration-2 focus-visible:ring-3 focus-visible:ring-ring">
          {art.artist}, <i>{art.title}</i>
        </a>
        , the Met
      </p>
    </section>
  );
}
