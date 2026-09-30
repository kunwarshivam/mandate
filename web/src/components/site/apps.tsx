import Image from "next/image";
import { cn } from "@/lib/utils";
import { OWL_SCROLL } from "./art";
import { MONO, PIXEL, SUNKEN } from "./letter";
import { ArtCredit } from "./wallpaper";

function Menu({ items }: { items: string[] }) {
  return (
    <div aria-hidden className={cn("flex shrink-0 gap-4 px-2 py-0.5 text-[0.9375rem]", PIXEL)}>
      {items.map((m) => (
        <span key={m}>
          <span className="underline">{m[0]}</span>
          {m.slice(1)}
        </span>
      ))}
    </div>
  );
}

export const README = `Welcome to Owlhead.

This is our homepage, set out as a desktop from the late 1990s.

Click an icon to open it. Drag a window by its title bar. The three buttons at its top right hide it, fill the screen with it, and close it. The taskbar brings back anything hidden, and Start has everything.

Owlhead is a trading agent for your own brokerage account. It works inside rules you write, and it writes down every decision it makes. It is in private beta, trading on paper.

The wallpapers are paintings and prints from The Metropolitan Museum of Art, which shares them as public domain. Pick one in Display.

Winamp is Webamp, the open source Winamp 2 for the browser. Its playlist is public domain recordings from Wikimedia Commons: Scott Joplin's The Entertainer, played by James Brigham; Maple Leaf Rag, by the US Air Force Strolling Strings; Sunflower Slow Drag, by the US Marine Band; Clair de Lune, by the US Air Force Wright Brass; and Chopin's Waltz in E minor, from Musopen.`;

/** Notepad, open on the readme. */
export function Notepad() {
  return (
    <>
      <Menu items={["File", "Edit", "Search", "Help"]} />
      <div className={cn(SUNKEN, "min-h-0 flex-1 overflow-y-auto bg-card px-3 py-2")}>
        <p className={cn(MONO, "text-[1.25rem] leading-[1.35] whitespace-pre-line")}>{README}</p>
      </div>
    </>
  );
}

/** A picture viewer, open on Soga Nichokuan's owl from the Met. */
export function PictureViewer() {
  return (
    <>
      <Menu items={["File", "View", "Help"]} />
      <figure className="flex min-h-0 flex-1 flex-col gap-2">
        <div className={cn(SUNKEN, "relative min-h-0 flex-1 bg-muted")}>
          <Image src={OWL_SCROLL.src} alt="An owl in ink, perched on a pine branch beneath a pale full moon, on a hanging scroll." fill sizes="(min-width: 640px) 22rem, 100vw" className="object-contain p-2" />
        </div>
        <figcaption className="px-1 pb-1">
          <ArtCredit art={OWL_SCROLL} />
        </figcaption>
      </figure>
    </>
  );
}
