/**
 * OKLCH to sRGB, WCAG 2 contrast, gamut limits, colour-vision-deficiency simulation and OKLab
 * distance, used by the design and palette pages and the contrast tests.
 */

export interface Oklch {
  l: number;
  c: number;
  h: number;
}

export type Rgb = [number, number, number];

export function parseOklch(css: string): Oklch {
  const match = /oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\)/.exec(css);
  if (!match) throw new Error(`not an opaque oklch() value: ${css}`);
  return { l: Number(match[1]), c: Number(match[2]), h: Number(match[3]) };
}

const round = (v: number, places: number) => Number(v.toFixed(places));

export function formatOklch({ l, c, h }: Oklch): string {
  return `oklch(${round(l, 3)} ${round(c, 3)} ${round(h, 1)})`;
}

/** Linear-light sRGB, not clipped: a channel outside 0..1 means the colour is out of gamut. */
function oklchToLinearUnclipped({ l, c, h }: Oklch): Rgb {
  const a = c * Math.cos((h * Math.PI) / 180);
  const b = c * Math.sin((h * Math.PI) / 180);
  const l_ = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m_ = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s_ = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
    -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
    -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_,
  ];
}

const clip = (v: number) => Math.min(1, Math.max(0, v));

/** Linear-light sRGB, clipped to the gamut. */
export function oklchToLinearSrgb(color: Oklch): Rgb {
  return oklchToLinearUnclipped(color).map(clip) as Rgb;
}

export function inGamut(color: Oklch, tolerance = 0.0005): boolean {
  return oklchToLinearUnclipped(color).every((v) => v >= -tolerance && v <= 1 + tolerance);
}

/** The most chroma sRGB can show at this lightness and hue. */
export function maxChroma(l: number, h: number): number {
  let lo = 0;
  let hi = 0.4;
  for (let i = 0; i < 30; i++) {
    const mid = (lo + hi) / 2;
    if (inGamut({ l, c: mid, h }, 0)) lo = mid;
    else hi = mid;
  }
  return lo;
}

export function relativeLuminance(color: Oklch): number {
  const [r, g, b] = oklchToLinearSrgb(color);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrastRatio(foreground: string, background: string): number {
  const a = relativeLuminance(parseOklch(foreground));
  const b = relativeLuminance(parseOklch(background));
  const [hi, lo] = a > b ? [a, b] : [b, a];
  return (hi + 0.05) / (lo + 0.05);
}

const encode = (v: number) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);
const decode = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

/** 8-bit sRGB channels. */
export function toRgb255(css: string): Rgb {
  return oklchToLinearSrgb(parseOklch(css)).map((v) => Math.round(encode(v) * 255)) as Rgb;
}

export function toHex(css: string): string {
  return `#${toRgb255(css)
    .map((v) => v.toString(16).padStart(2, "0"))
    .join("")}`;
}

/** A translucent colour composited over an opaque one, returned as 8-bit sRGB. */
export function composite(css: string, alpha: number, over: string): Rgb {
  const fg = toRgb255(css);
  const bg = toRgb255(over);
  return fg.map((v, i) => Math.round(v * alpha + bg[i] * (1 - alpha))) as Rgb;
}

/** WCAG 2 contrast between two 8-bit sRGB colours. */
export function rgbContrast(a: Rgb, b: Rgb): number {
  const y = (rgb: Rgb) => {
    const [r, g, bl] = rgb.map((v) => decode(v / 255));
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi, lo] = y(a) > y(b) ? [y(a), y(b)] : [y(b), y(a)];
  return (hi + 0.05) / (lo + 0.05);
}

export function hexOf(rgb: Rgb): string {
  return `#${rgb.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

function linearToOklab([r, g, b]: Rgb): Rgb {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

export type Vision = "normal" | "deuteranopia" | "protanopia" | "tritanopia";

/**
 * Machado, Oliveira and Fernandes (2009), severity 1.0, applied to linear-light sRGB. Deuteranopia
 * and protanopia are the red-green deficiencies (about 1 in 12 men of northern European descent);
 * tritanopia is the rare blue-yellow one.
 */
const CVD_MATRIX: Record<Exclude<Vision, "normal">, [Rgb, Rgb, Rgb]> = {
  protanopia: [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
  ],
  deuteranopia: [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.01182, 0.04294, 0.968881],
  ],
  tritanopia: [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.3039],
  ],
};

function simulateLinear(rgb: Rgb, vision: Vision): Rgb {
  if (vision === "normal") return rgb;
  const m = CVD_MATRIX[vision];
  return m.map((row) => clip(row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2])) as Rgb;
}

/** Linear sRGB after rounding to 8-bit, as a screen would show it. */
function screenLinear(css: string): Rgb {
  return toRgb255(css).map((v) => decode(v / 255)) as Rgb;
}

/** The colour as someone with this vision would see it, as 8-bit sRGB hex. */
export function simulateHex(css: string, vision: Vision): string {
  return hexOf(simulateLinear(screenLinear(css), vision).map((v) => Math.round(encode(v) * 255)) as Rgb);
}

/**
 * Euclidean distance in OKLab between two colours as seen with this vision. About 0.02 is a just
 * noticeable difference side by side; this system asks 0.1 for two things that must never be confused.
 */
export function deltaEOK(a: string, b: string, vision: Vision = "normal"): number {
  const [l1, a1, b1] = linearToOklab(simulateLinear(screenLinear(a), vision));
  const [l2, a2, b2] = linearToOklab(simulateLinear(screenLinear(b), vision));
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2);
}
