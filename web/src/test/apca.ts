/**
 * APCA for the contrast tests only. apca-w3 is licensed for WCAG contrast checks of web content,
 * and its `colorparsley` dependency is AGPL-3.0, so both stay dev dependencies: lint bans the import
 * outside tests, and `scripts/no-apca.mjs` fails the build if APCA code reaches `.next`.
 */
import { APCAcontrast, sRGBtoY } from "apca-w3";
import { toRgb255 } from "@/lib/color";
import { type PairKind, REQUIREMENT } from "@/lib/contrast-pairs";

export function apcaLc(foreground: string, background: string): number {
  return Number(APCAcontrast(sRGBtoY(toRgb255(foreground)), sRGBtoY(toRgb255(background))));
}

export function passesApca(lc: number, kind: PairKind): boolean {
  return Math.abs(lc) >= REQUIREMENT[kind].apca;
}
