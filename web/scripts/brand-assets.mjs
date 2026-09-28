#!/usr/bin/env node
/**
 * Generates the favicon, app icons, Open Graph image, and web manifest in `public/` from the brand
 * sources (DEC-203): `src/components/brand/owlhead-mark.svg` and `brand/og-image.svg`. Run with
 * `npm run brand` and commit the output; `brand-assets.test.ts` regenerates into a temporary
 * directory and fails when the committed files differ.
 */
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Resvg } from "@resvg/resvg-js";

const WEB = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const MARK_SOURCE = path.join(WEB, "src/components/brand/owlhead-mark.svg");
export const OG_SOURCE = path.join(WEB, "brand/og-image.svg");
export const PUBLIC_DIR = path.join(WEB, "public");

export const NAVY = "#183D73";
export const OFF_WHITE = "#F7FAFE";

const FAVICON_VIEWBOX = "-40 -8 530 530";
const FAVICON_PNG_SIZES = [16, 32, 48];
const TILE = 1000;
/** The mark's height on a filled tile, as a share of the tile. */
const TILE_MARK_HEIGHT = 0.76;
/** A maskable icon's safe zone is the centred circle whose diameter is 80% of the tile. */
const MASKABLE_SAFE_RADIUS = 0.4;
/** Room between the mark's farthest point and the safe zone's edge. */
const MASKABLE_MARGIN = 0.96;

export const OUTPUTS = [
  "favicon.svg",
  "favicon-16.png",
  "favicon-32.png",
  "favicon-48.png",
  "favicon.ico",
  "apple-touch-icon.png",
  "pwa-192.png",
  "pwa-512.png",
  "pwa-maskable-512.png",
  "og-image.png",
  "site.webmanifest",
];

function markPath(svg) {
  const d = /<path\b[^>]*\sd="([^"]+)"/.exec(svg)?.[1];
  if (!d) throw new Error("owlhead-mark.svg has no path data");
  return d;
}

/** The mark's path is absolute points only, so its points are its outline. */
function markPoints(d) {
  return [...d.matchAll(/(-?\d+(?:\.\d+)?),(-?\d+(?:\.\d+)?)/g)].map((m) => [Number(m[1]), Number(m[2])]);
}

function bounds(points) {
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  return { minX: Math.min(...xs), maxX: Math.max(...xs), minY: Math.min(...ys), maxY: Math.max(...ys) };
}

const round = (n) => Number(n.toFixed(4));

function faviconSvg(d) {
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${FAVICON_VIEWBOX}">` +
    `<style>path{fill:${NAVY}}@media (prefers-color-scheme:dark){path{fill:${OFF_WHITE}}}</style>` +
    `<path fill-rule="evenodd" d="${d}"/></svg>\n`
  );
}

/** The off-white mark on a navy square, its bounding box centred and scaled to `scale` tile units. */
function tileSvg(d, scale) {
  const b = bounds(markPoints(d));
  const tx = round(TILE / 2 - ((b.minX + b.maxX) / 2) * scale);
  const ty = round(TILE / 2 - ((b.minY + b.maxY) / 2) * scale);
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${TILE} ${TILE}">` +
    `<rect width="${TILE}" height="${TILE}" fill="${NAVY}"/>` +
    `<path fill="${OFF_WHITE}" fill-rule="evenodd" transform="translate(${tx} ${ty}) scale(${round(scale)})" d="${d}"/></svg>`
  );
}

function tileScale(d) {
  const b = bounds(markPoints(d));
  return (TILE * TILE_MARK_HEIGHT) / (b.maxY - b.minY);
}

/** The largest scale at which every point of the mark stays inside the maskable safe circle. */
function maskableScale(d) {
  const points = markPoints(d);
  const b = bounds(points);
  const cx = (b.minX + b.maxX) / 2;
  const cy = (b.minY + b.maxY) / 2;
  const reach = Math.max(...points.map(([x, y]) => Math.hypot(x - cx, y - cy)));
  return (TILE * MASKABLE_SAFE_RADIUS * MASKABLE_MARGIN) / reach;
}

function png(svg, width) {
  return new Resvg(svg, { fitTo: { mode: "width", value: width }, font: { loadSystemFonts: false } }).render().asPng();
}

/** An ICO container of PNG images (supported by every current browser), smallest first. */
export function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  const entries = [];
  let offset = 6 + 16 * images.length;
  for (const { size, data } of images) {
    const entry = Buffer.alloc(16);
    entry.writeUInt8(size >= 256 ? 0 : size, 0);
    entry.writeUInt8(size >= 256 ? 0 : size, 1);
    entry.writeUInt8(0, 2);
    entry.writeUInt8(0, 3);
    entry.writeUInt16LE(1, 4);
    entry.writeUInt16LE(32, 6);
    entry.writeUInt32LE(data.length, 8);
    entry.writeUInt32LE(offset, 12);
    entries.push(entry);
    offset += data.length;
  }
  return Buffer.concat([header, ...entries, ...images.map((i) => i.data)]);
}

function manifest() {
  const icon = (src, size, purpose) => ({ src, sizes: `${size}x${size}`, type: "image/png", purpose });
  return `${JSON.stringify(
    {
      name: "Owlhead",
      short_name: "Owlhead",
      start_url: "/",
      display: "standalone",
      theme_color: NAVY,
      background_color: OFF_WHITE,
      icons: [icon("/pwa-192.png", 192, "any"), icon("/pwa-512.png", 512, "any"), icon("/pwa-maskable-512.png", 512, "maskable")],
    },
    null,
    2,
  )}\n`;
}

/** Writes every file in `OUTPUTS` to `outDir` and returns their names. */
export async function generateBrandAssets(outDir = PUBLIC_DIR) {
  const d = markPath(await readFile(MARK_SOURCE, "utf8"));
  const og = await readFile(OG_SOURCE, "utf8");
  const favicon = faviconSvg(d);
  const tile = tileSvg(d, tileScale(d));
  const maskable = tileSvg(d, maskableScale(d));
  const favicons = FAVICON_PNG_SIZES.map((size) => ({ size, data: png(favicon, size) }));

  const files = new Map([
    ["favicon.svg", favicon],
    ...favicons.map(({ size, data }) => [`favicon-${size}.png`, data]),
    ["favicon.ico", ico(favicons)],
    ["apple-touch-icon.png", png(tile, 180)],
    ["pwa-192.png", png(tile, 192)],
    ["pwa-512.png", png(tile, 512)],
    ["pwa-maskable-512.png", png(maskable, 512)],
    ["og-image.png", png(og, 1200)],
    ["site.webmanifest", manifest()],
  ]);
  await mkdir(outDir, { recursive: true });
  for (const [name, data] of files) await writeFile(path.join(outDir, name), data);
  return [...files.keys()];
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const written = await generateBrandAssets(process.argv[2] ? path.resolve(process.argv[2]) : PUBLIC_DIR);
  console.log(`wrote ${written.length} files`);
}
