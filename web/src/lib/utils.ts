import { createCn } from "cn/config";

/** The type scale's custom sizes (globals.css `--text-*`) must merge as font sizes, not colours. */
export const cn = createCn({
  extend: { classGroups: { "font-size": [{ text: ["hero", "h1", "h2", "h3", "figure", "caption", "label"] }] } },
});
