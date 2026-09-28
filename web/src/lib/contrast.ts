/**
 * WCAG 2.2 contrast for the palette's semantic pairs, and the colour-vision checks, for either theme (web/COLOR.md).
 * Used by the dev-only `/palette` route and the tests. APCA is measured only in the tests
 * (`src/test/apca.ts`): apca-w3's licence covers that use, so no app code may import it.
 */
import { contrastRatio, deltaEOK } from "./color";
import { CVD_CHECKS, CVD_DISTINCT, CVD_VISIONS, type CvdCheck, PAIRS, type Pair, type PairKind, REQUIREMENT } from "./contrast-pairs";
import { PALETTE, type Palette } from "./palette";

export interface Measure {
  ratio: number;
  passWcag: boolean;
}

export function measure(fg: string, bg: string, kind: PairKind): Measure {
  const ratio = contrastRatio(fg, bg);
  return { ratio, passWcag: ratio >= REQUIREMENT[kind].wcag };
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
  /** Whether this theme relies on the pair staying apart. */
  requiredHere: boolean;
}

export function checkCvd(palette: Palette = PALETTE): CvdResult[] {
  return CVD_CHECKS.map((check) => {
    const a = palette.tokens[check.a].value;
    const b = palette.tokens[check.b].value;
    const byVision = Object.fromEntries(CVD_VISIONS.map((v) => [v, deltaEOK(a, b, v)]));
    return {
      ...check,
      normal: deltaEOK(a, b),
      byVision,
      pass: Object.values(byVision).every((d) => d >= CVD_DISTINCT),
      requiredHere: check.required.includes(palette.theme),
    };
  });
}
