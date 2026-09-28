/**
 * The brand's colours, taken from the Ink and Gold palette (DEC-204) so the two cannot drift.
 * `scripts/brand-assets.mjs` writes the ink and off-white as literals; `brand-assets.test.ts` holds
 * them equal.
 */
import { toHex } from "./color";
import { PALETTE, PALETTE_DARK, type Palette, type TokenName } from "./palette";

const hex = (palette: Palette, token: TokenName) => toHex(palette.tokens[token].value).toUpperCase();

export const BRAND_PALETTE = [
  { name: "Ink", hex: hex(PALETTE, "foreground"), role: "The mark and the type on a light surface, and the primary action" },
  { name: "Off-white", hex: hex(PALETTE, "card"), role: "The mark on a dark surface; the tile behind the app icons and the share image" },
  { name: "Gold", hex: hex(PALETTE, "mandate-marker"), role: "The one accent: your mandate's rules and markers, and the account's line. Never text, never a block" },
  { name: "Dark gold", hex: hex(PALETTE, "mandate-strong"), role: "Gold as text on a light surface" },
  { name: "Gold tint", hex: hex(PALETTE, "mandate"), role: "Your mandate's field" },
  { name: "Night", hex: hex(PALETTE_DARK, "background"), role: "The page in dark mode" },
] as const;

export type BrandColorName = (typeof BRAND_PALETTE)[number]["name"];

export function brandHex(name: BrandColorName): string {
  const color = BRAND_PALETTE.find((c) => c.name === name);
  if (!color) throw new Error(`unknown brand colour ${name}`);
  return color.hex;
}

export const INK = brandHex("Ink");
export const OFF_WHITE = brandHex("Off-white");
export const NIGHT = brandHex("Night");

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const [r, g, b] = channels.map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG 2.2 contrast ratio of two `#RRGGBB` colours. */
export function hexContrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
