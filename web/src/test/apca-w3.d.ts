declare module "apca-w3" {
  /** Lc, signed: positive for dark text on light, negative for light text on dark. */
  export function APCAcontrast(textY: number, backgroundY: number, places?: number): number | string;
  export function sRGBtoY(rgb: [number, number, number] | number[]): number;
}
