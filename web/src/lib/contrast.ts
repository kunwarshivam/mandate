/**
 * WCAG 2.2 contrast and APCA lightness contrast for the palette's semantic pairs, and the
 * colour-vision checks (web/COLOR.md). Used by the dev-only `/palette` route and the tests.
 */
import { APCAcontrast, sRGBtoY } from "apca-w3";
import { contrastRatio, deltaEOK, toRgb255 } from "./color";
import { CVD_CHECKS, CVD_DISTINCT, CVD_VISIONS, type CvdCheck, PAIRS, type Pair, type PairKind, REQUIREMENT } from "./contrast-pairs";
import { PALETTE, type Palette } from "./palette";

export function apcaLc(foreground: string, background: string): number {
  return Number(APCAcontrast(sRGBtoY(toRgb255(foreground)), sRGBtoY(toRgb255(background))));
}

export interface Measure {
  ratio: number;
  lc: number;
  passWcag: boolean;
  passApca: boolean;
}

export function measure(fg: string, bg: string, kind: PairKind): Measure {
  const ratio = contrastRatio(fg, bg);
  const lc = apcaLc(fg, bg);
  const need = REQUIREMENT[kind];
  return { ratio, lc, passWcag: ratio >= need.wcag, passApca: Math.abs(lc) >= need.apca };
}

export type PairResult = Pair & Measure;

export function checkPair(pair: Pair, palette: Palette = PALETTE): PairResult {
  return { ...pair, ...measure(palette.tokens[pair.fg].value, palette.tokens[pair.bg].value, pair.kind) };
}

export function checkPalette(palette: Palette = PALETTE): PairResult[] {
  return PAIRS.map((p) => checkPair(p, palette));
}

export interface CvdResult extends CvdCheck {
  normal: number;
  byVision: Record<string, number>;
  pass: boolean;
}

export function checkCvd(palette: Palette = PALETTE): CvdResult[] {
  return CVD_CHECKS.map((check) => {
    const a = palette.tokens[check.a].value;
    const b = palette.tokens[check.b].value;
    const byVision = Object.fromEntries(CVD_VISIONS.map((v) => [v, deltaEOK(a, b, v)]));
    return { ...check, normal: deltaEOK(a, b), byVision, pass: Object.values(byVision).every((d) => d >= CVD_DISTINCT) };
  });
}
