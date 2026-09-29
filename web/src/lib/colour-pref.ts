/**
 * The colour-blind friendly preference (web/COLOR.md): a gain turns blue, and a loss raspberry in light or orange in dark.
 * Outside `next dev` the app always renders the default gain and loss colours; the preference moves
 * to settings once there is a settings store.
 */
export const CVD_COOKIE = "mandate-cvd";
export const CVD_PARAM = "cvd";

export const colourBlindEnabled = process.env.NODE_ENV === "development";

export function cvdFromParam(value: string): "on" | "off" {
  return value === "1" || value === "on" ? "on" : "off";
}
