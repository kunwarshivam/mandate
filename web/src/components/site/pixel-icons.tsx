import { cn } from "@/lib/utils";
import styles from "./letter.module.css";

/**
 * Desktop icons, drawn as 16 by 16 sprites the way 1990s icons were, one character to a pixel. They
 * sit on a painting, so their outline stays dark and their paper stays light in both themes.
 */
const INK: Record<string, string> = {
  k: "var(--icon-line)",
  w: "var(--icon-paper)",
  g: "var(--muted-foreground)",
  b: "var(--series-1)",
  t: "var(--series-3)",
  s: "var(--series-4)",
  y: "var(--highlight)",
};

export type Sprite = readonly string[];

export const BOOK: Sprite = [
  "................",
  "..kkkkkkkkkkk...",
  ".kbbbbbbbbbbbk..",
  ".kbkkkkkkkkbbk..",
  ".kbkyyyyyykbbk..",
  ".kbkkkkkkkkbbk..",
  ".kbbbbbbbbbbbk..",
  ".kbbbbbbbbbbbk..",
  ".kbbbbbbbbbbbk..",
  ".kbbbbbbbbbbbk..",
  ".kbbbbbbbbbbbk..",
  ".kbbbbbbbbbbbkk.",
  ".kkkkkkkkkkkkkwk",
  "..kwwwwwwwwwwwwk",
  "...kkkkkkkkkkkk.",
  "................",
];

export const NOTE: Sprite = [
  "................",
  "...kkkkkkkk.....",
  "...kwwwwwwkk....",
  "...kwwwwwwkwk...",
  "...kwggggwkkkk..",
  "...kwwwwwwwwwk..",
  "...kwggggggwwk..",
  "...kwwwwwwwwwk..",
  "...kwggggggwwk..",
  "...kwwwwwwwwwk..",
  "...kwggggwwwwk..",
  "...kwwwwwwwwwk..",
  "...kwwwwwwwwwk..",
  "...kwwwwwwwwwk..",
  "...kkkkkkkkkkk..",
  "................",
];

export const LEDGER: Sprite = [
  "................",
  "...kkkkkkkk.....",
  "...kwwwwwwkk....",
  "...kwwwwwwkwk...",
  "...kwttttwkkkk..",
  "...kwwwwwwwwwk..",
  "...kwttttttwwk..",
  "...kwwwwwwwwwk..",
  "...kwttttttwwk..",
  "...kwwwwwwwwwk..",
  "...kwttttwkkkkk.",
  "...kwwwwwkyyyyk.",
  "...kwwwwwkyyyyk.",
  "...kwwwwwkyyyyk.",
  "...kkkkkkkkkkkk.",
  "................",
];

export const HELP: Sprite = [
  "................",
  "....kkkkkkkk....",
  "...kbbbbbbbbk...",
  "..kbbbwwwwbbbk..",
  "..kbbwwkkwwbbk..",
  "..kbbkkbbwwbbk..",
  "..kbbbbbwwkbbk..",
  "..kbbbbwwkbbbk..",
  "..kbbbbwwkbbbk..",
  "..kbbbbbkbbbbk..",
  "..kbbbbwwbbbbk..",
  "...kbbbwwkbbk...",
  "....kkbbbbkk....",
  "......kbbk......",
  ".......kk.......",
  "................",
];

export const KEY: Sprite = [
  "................",
  "................",
  "................",
  "................",
  "..kkkk..........",
  ".kyyyyk.........",
  "kyykkyyk........",
  "kyk..kykkkkkkkk.",
  "kyk..kyyyyyyyyyk",
  "kyykkyykkkykykyk",
  ".kyyyyk...kkkkk.",
  "..kkkk..........",
  "................",
  "................",
  "................",
  "................",
];

export const MONITOR: Sprite = [
  "................",
  ".kkkkkkkkkkkkkk.",
  ".kwwwwwwwwwwwwk.",
  ".kwkkkkkkkkkkwk.",
  ".kwksssssyyskwk.",
  ".kwksssssyyskwk.",
  ".kwksssssssskwk.",
  ".kwksssttssskwk.",
  ".kwksstttttskwk.",
  ".kwkttttttttkwk.",
  ".kwkkkkkkkkkkwk.",
  ".kwwwwwwwwwwwwk.",
  ".kkkkkkkkkkkkkk.",
  "......kwwk......",
  "....kkkkkkkk....",
  "................",
];

export const PICTURE: Sprite = [
  "................",
  ".kkkkkkkkkkkkkk.",
  ".kwwwwwwwwwwwwk.",
  ".kwkkkkkkkkkkwk.",
  ".kwkssssssyykwk.",
  ".kwkskssksyykwk.",
  ".kwkskkkkssskwk.",
  ".kwkswkkwssskwk.",
  ".kwkskkkkssskwk.",
  ".kwkttttttttkwk.",
  ".kwkkkkkkkkkkwk.",
  ".kwwwwwwwwwwwwk.",
  ".kkkkkkkkkkkkkk.",
  "................",
  "................",
  "................",
];

export const BOLT: Sprite = [
  "................",
  ".kkkkkkkkkkkkkk.",
  ".kkkkkkkkyykkkk.",
  ".kkkkkkkyykkkkk.",
  ".kkkkkkyykkkkkk.",
  ".kkkkkyyykkkkkk.",
  ".kkkkyyykkkkkkk.",
  ".kkkyyyyyyykkkk.",
  ".kkkkkkyyykkkkk.",
  ".kkkkkyyykkkkkk.",
  ".kkkkyyykkkkkkk.",
  ".kkkyykkkkkkkkk.",
  ".kkkykkkkkkkkkk.",
  ".kkkkkkkkkkkkkk.",
  ".kkkkkkkkkkkkkk.",
  "................",
];

const BASKET: Sprite = [
  "..kkkkkkkkkkkk..",
  "..kwgwgwgwgwgk..",
  "..kkkkkkkkkkkk..",
  "...kwgwgwgwgk...",
  "...kwgwgwgwgk...",
  "...kwgtttwwgk...",
  "...kwtwgwtwgk...",
  "...kwgwttwwgk...",
  "...kwgwgwgwgk...",
  "...kwgwgwgwgk...",
  "....kwgwgwgk....",
  "....kkkkkkkk....",
  "................",
];

export const BIN: Sprite = ["................", "....ww..w.ww....", "...wwkwwkwwkw...", ...BASKET];

export const BIN_EMPTY: Sprite = ["................", "................", "................", ...BASKET];

export const FILM: Sprite = [
  "................",
  ".kkkkkkkkkkkkkk.",
  ".kwkkwkkwkkwkwk.",
  ".kkkkkkkkkkkkkk.",
  ".kssssssssssssk.",
  ".ksssswwssssssk.",
  ".ksssswwwsssssk.",
  ".ksssswwwwssssk.",
  ".ksssswwwsssssk.",
  ".ksssswwssssssk.",
  ".kttttttttttttk.",
  ".kkkkkkkkkkkkkk.",
  ".kwkkwkkwkkwkwk.",
  ".kkkkkkkkkkkkkk.",
  "................",
  "................",
];

/** A compact Mac, smiling on its screen as one did at start-up. */
export const MAC: Sprite = [
  "................",
  "..kkkkkkkkkkkk..",
  "..kwwwwwwwwwwk..",
  "..kwkkkkkkkkwk..",
  "..kwkwwwwwwkwk..",
  "..kwkwkwwkwkwk..",
  "..kwkwwwwwwkwk..",
  "..kwkwkkkkwkwk..",
  "..kwkwwwwwwkwk..",
  "..kwkkkkkkkkwk..",
  "..kwwwwwwwwwwk..",
  "..kwwwwwggggwk..",
  "..kwwwwwwwwwwk..",
  "..kkkkkkkkkkkk..",
  "..kggggggggggk..",
  "..kkkkkkkkkkkk..",
];

/** A PC of the time: a monitor on a desktop case, its screen the colour of a fresh Windows install. */
export const PC: Sprite = [
  "................",
  "..kkkkkkkkkkkk..",
  "..kwwwwwwwwwwk..",
  "..kwkkkkkkkkwk..",
  "..kwkttttttkwk..",
  "..kwkttttttkwk..",
  "..kwkttttttkwk..",
  "..kwkkkkkkkkwk..",
  "..kwwwwwwwwwwk..",
  "..kkkkkkkkkkkk..",
  ".....kkkkkk.....",
  ".kkkkkkkkkkkkkk.",
  ".kwwwwwwwwwwwwk.",
  ".kwgggwwwwwwtwk.",
  ".kkkkkkkkkkkkkk.",
  "................",
];

/** The Mac's hard disk, which sat at the top right of every desktop. */
export const DISK: Sprite = [
  "................",
  "................",
  "................",
  "................",
  "................",
  ".kkkkkkkkkkkkkk.",
  ".kwwwwwwwwwwwwk.",
  ".kwwwwwwwwwwwwk.",
  ".kwwwwwwwwwwwwk.",
  ".kwkkkkwwwwwtwk.",
  ".kwwwwwwwwwwwwk.",
  ".kkkkkkkkkkkkkk.",
  ".kggggggggggggk.",
  ".kkkkkkkkkkkkkk.",
  "................",
  "................",
];

/** One run of same-coloured pixels per rect, so a sprite is a few dozen rects. */
function runs(sprite: Sprite) {
  const out: { x: number; y: number; w: number; fill: string }[] = [];
  sprite.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const c = row[x];
      let end = x + 1;
      while (end < row.length && row[end] === c) end++;
      if (INK[c]) out.push({ x, y, w: end - x, fill: INK[c] });
      x = end;
    }
  });
  return out;
}

export function PixelIcon({ sprite, className }: { sprite: Sprite; className?: string }) {
  return (
    <svg aria-hidden viewBox="0 0 16 16" shapeRendering="crispEdges" className={cn(styles.icon, "size-8 shrink-0", className)}>
      {runs(sprite).map((r) => (
        <rect key={`${r.x}.${r.y}`} x={r.x} y={r.y} width={r.w} height={1} fill={r.fill} />
      ))}
    </svg>
  );
}
