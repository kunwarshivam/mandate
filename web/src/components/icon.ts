import { createElement, type JSX, type SVGProps } from "react";

/**
 * One Pixelarticons glyph (DEC-475), imported one file per icon:
 * `import { Home } from "pixelarticons/react/Home.js"`. Each is drawn on a 24px grid without
 * anti-aliasing, so it renders at 24px (`size-6`, its default) or 48px (`size-12`) and never in
 * between, where its pixels would blur. The glyph sits inside the square with room around it, so a
 * 24px pixel icon reads at the size a 16px outline icon did.
 */
export type Icon = (props: SVGProps<SVGSVGElement>) => JSX.Element;

/**
 * Stop's octagon (DEC-206), which Pixelarticons does not draw: a solid stepped octagon on the same
 * 24px grid, 16px across, as heavy as the set's solid glyphs.
 */
export const StopOctagon: Icon = (props) =>
  createElement(
    "svg",
    { viewBox: "0 0 24 24", width: "24", height: "24", fill: "currentColor", xmlns: "http://www.w3.org/2000/svg", ...props },
    createElement("path", { d: "M8 4h8v2h2v2h2v8h-2v2h-2v2H8v-2H6v-2H4V8h2V6h2V4Z" }),
  );

/**
 * A Kumo menu draws an icon component at 16px, between the grid's pixels, so menus get the icon as
 * an element at 24px, its negative margin keeping the row as tall as before.
 */
export function menuIcon(icon: Icon): JSX.Element {
  return createElement(icon, { className: "mr-2 -my-1 size-6 shrink-0", "aria-hidden": true });
}
