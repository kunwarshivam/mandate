import { createCn } from "cn/config";

/** The type scale's custom sizes (globals.css `--text-*`) must merge as font sizes, not colours. */
export const cn = createCn({
  extend: { classGroups: { "font-size": [{ text: ["display", "title", "heading", "caption"] }] } },
});
