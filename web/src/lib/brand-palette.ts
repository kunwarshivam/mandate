/** The founder's "navy and brass" palette (DEC-202). `scripts/brand-assets.mjs` uses the same values. */
export const BRAND_PALETTE = [
  { name: "Navy", hex: "#183D73", role: "The brand; the mark, always on a light surface" },
  { name: "Brass", hex: "#AC7D1B", role: "The one accent: rules, borders, large type. Never body text" },
  { name: "Dark brass", hex: "#634606", role: "Brass as text, and text on the brass tint" },
  { name: "Brass tint", hex: "#FDF1DC", role: "A brass notice field" },
  { name: "Slate ink", hex: "#181C21", role: "Text" },
  { name: "Off-white", hex: "#F7FAFE", role: "The page; the tile behind the app icons and the share image" },
] as const;

export type BrandColorName = (typeof BRAND_PALETTE)[number]["name"];

export function brandHex(name: BrandColorName): string {
  const color = BRAND_PALETTE.find((c) => c.name === name);
  if (!color) throw new Error(`unknown brand colour ${name}`);
  return color.hex;
}

export const NAVY = brandHex("Navy");
export const OFF_WHITE = brandHex("Off-white");

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
