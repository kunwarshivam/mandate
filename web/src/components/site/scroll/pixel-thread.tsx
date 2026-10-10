"use client";

import { useEffect, useRef } from "react";
import { perchOn } from "./owl-flight";
import { type Spring, stepSpring } from "./spring";
import styles from "./scroll.module.css";

/**
 * The long page's thread (DEC-907): a line of square pixels in tide that sews the page together,
 * knotted at every place the owl lands. It runs down the outer gutter, crosses each part in the
 * space above it, passes behind the words and under the pictures and the painting, and is sewn as
 * far as the visitor has read, catching up on a spring. With motion reduced it is all there at once.
 */

export interface Point {
  x: number;
  y: number;
}

/** A place the thread is knotted, over the top edge of what the owl lands on, and how far down that reaches. */
export interface Knot extends Point {
  below: number;
}

/** Pixels a stitch, on a phone and wider. */
export function cellSize(width: number): number {
  return width < 640 ? 6 : 8;
}

/** How far into the outer gutter the thread runs, beside the page's widest column and its padding. */
export function gutterX(width: number, cell: number): number {
  const pad = width < 640 ? 20 : width < 1024 ? 32 : 48;
  const outer = Math.max(0, (width - 1280) / 2) + pad;
  return Math.max(cell, Math.round((outer * 0.4) / cell) * cell);
}

/** The thread's stitches run three on and one off; the knots are solid. */
export const isStitch = (i: number) => i % 4 !== 3;

/**
 * The thread's path through each knot in turn, in straight runs: down under what the knot sits on,
 * out to its own gutter below it, down, across the space above the next part when the next knot is
 * on the other side, down again and in to the knot. It only ever goes down or across, so it never
 * runs over itself and how far it is sewn is a height.
 */
export function route(knots: Knot[], crossings: number[], width: number, gutter: number): Point[] {
  const side = (p: Point) => (p.x < width / 2 ? gutter : width - gutter);
  const path: Point[] = [];
  knots.forEach((k, i) => {
    const from = knots[i - 1];
    if (from) {
      const out = side(from);
      const into = side(k);
      const cross = Math.max(crossings[i]!, from.below);
      path.push({ x: out, y: from.below });
      if (out !== into && cross < k.y) path.push({ x: out, y: cross }, { x: into, y: cross });
      path.push({ x: into, y: k.y });
    }
    path.push({ x: k.x, y: k.y });
    if (i < knots.length - 1) path.push({ x: k.x, y: k.below });
  });
  return path.filter((p, i) => i === 0 || p.x !== path[i - 1]!.x || p.y !== path[i - 1]!.y);
}

/** The path's corners rounded to `radius` and walked a pixel at a time. */
function walk(path: Point[], radius: number): Point[] {
  const out: Point[] = [];
  const line = (a: Point, b: Point) => {
    const n = Math.max(1, Math.ceil(Math.hypot(b.x - a.x, b.y - a.y)));
    for (let s = 0; s <= n; s++) out.push({ x: a.x + ((b.x - a.x) * s) / n, y: a.y + ((b.y - a.y) * s) / n });
  };
  let start = path[0];
  if (!start) return out;
  for (let i = 1; i < path.length; i++) {
    const b = path[i]!;
    const c = path[i + 1];
    if (!c) {
      line(start, b);
      break;
    }
    const inLen = Math.hypot(b.x - start.x, b.y - start.y);
    const outLen = Math.hypot(c.x - b.x, c.y - b.y);
    const r = Math.min(radius, inLen / 2, outLen / 2);
    const enter = { x: b.x - ((b.x - start.x) / inLen) * r, y: b.y - ((b.y - start.y) / inLen) * r };
    const leave = { x: b.x + ((c.x - b.x) / outLen) * r, y: b.y + ((c.y - b.y) / outLen) * r };
    line(start, enter);
    const steps = Math.max(2, Math.ceil(r * 1.6));
    for (let s = 1; s <= steps; s++) {
      const t = s / steps;
      const u = 1 - t;
      out.push({ x: u * u * enter.x + 2 * u * t * b.x + t * t * leave.x, y: u * u * enter.y + 2 * u * t * b.y + t * t * leave.y });
    }
    start = leave;
  }
  return out;
}

/**
 * The path as whole stitches on a grid of `cell` pixels: one square per step, each touching the last
 * on a side or a corner, never doubling back and never turning a square corner, as a hand-drawn
 * pixel line does.
 */
export function rasterize(path: Point[], cell: number, radius = cell * 2): Point[] {
  const cells: Point[] = [];
  for (const p of walk(path, radius)) {
    const c = { x: Math.floor(p.x / cell), y: Math.floor(p.y / cell) };
    const last = cells[cells.length - 1];
    if (last && last.x === c.x && last.y === c.y) continue;
    const before = cells[cells.length - 2];
    if (before && last && Math.abs(before.x - c.x) === 1 && Math.abs(before.y - c.y) === 1) cells.pop();
    cells.push(c);
  }
  return cells;
}

/** The parts of the page from top to bottom, in page pixels, each with the thread's colour there. */
interface Band {
  top: number;
  bottom: number;
  colour: string;
}

export function PixelThread() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    const page = canvas?.closest<HTMLElement>("[data-slot=long-page]");
    const g = canvas?.getContext("2d");
    if (!canvas || !page || !g) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sewn: Spring = { x: 0, v: 0 };
    let raf = 0;
    let last = 0;
    let colours = new Map<Element, string>();

    const readColours = () => {
      colours = new Map([...page.querySelectorAll(":scope > section, :scope > footer")].map((el) => [el, getComputedStyle(el).getPropertyValue("--thread").trim()]));
    };

    const size = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(window.innerWidth * dpr);
      canvas.height = Math.round(window.innerHeight * dpr);
      canvas.style.width = `${window.innerWidth}px`;
      canvas.style.height = `${window.innerHeight}px`;
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
    };

    const layout = () => {
      const box = page.getBoundingClientRect();
      const width = box.width;
      const cell = cellSize(width);
      const snap = (v: number) => (Math.floor(v / cell) + 0.5) * cell;
      const perches = [...page.querySelectorAll<HTMLElement>("[data-perch]")].filter((el) => el.getBoundingClientRect().width > 0);
      const knots = perches.map((el) => {
        const p = perchOn(el, 0, box);
        return { x: snap(p.x), y: snap(p.y - cell * 2), below: snap(el.getBoundingClientRect().bottom - box.top + cell * 2) };
      });
      const crossings = perches.map((el) => {
        const part = el.closest("section, footer");
        return snap((part ? part.getBoundingClientRect().top - box.top : 0) + cell * 3);
      });
      const cells = rasterize(route(knots, crossings, width, gutterX(width, cell) + cell / 2), cell);
      const bands: Band[] = [...colours].map(([el, colour]) => {
        const r = el.getBoundingClientRect();
        return { top: r.top - box.top, bottom: r.bottom - box.top, colour };
      });
      return { box, cell, cells, knots, bands, fallback: getComputedStyle(page).getPropertyValue("--thread").trim() };
    };

    const paint = (upTo: number) => {
      const { box, cell, cells, knots, bands, fallback } = layout();
      const vh = window.innerHeight;
      g.clearRect(0, 0, window.innerWidth, vh);
      const colourAt = (y: number) => bands.find((b) => y >= b.top && y < b.bottom)?.colour || fallback;
      const visible = (y: number) => y + box.top > -cell && y + box.top < vh && y <= upTo;
      cells.forEach((c, i) => {
        const y = c.y * cell;
        if (!isStitch(i) || !visible(y)) return;
        g.fillStyle = colourAt(y);
        g.fillRect(box.left + c.x * cell, box.top + y, cell, cell);
      });
      for (const k of knots) {
        const y = k.y - cell * 1.5;
        if (!visible(k.y)) continue;
        g.fillStyle = colourAt(k.y);
        g.fillRect(box.left + k.x - cell * 1.5, box.top + y, cell * 3, cell * 3);
      }
      canvas.dataset.thread = upTo === Infinity ? "whole" : "sewing";
    };

    const target = () => -page.getBoundingClientRect().top + window.innerHeight * 0.55;

    const frame = (now: number) => {
      const dt = last ? (now - last) / 1000 : 0;
      last = now;
      const goal = target();
      stepSpring(sewn, goal, dt, { stiffness: 90, damping: 16 });
      paint(sewn.x);
      raf = Math.abs(goal - sewn.x) > 0.5 || Math.abs(sewn.v) > 1 ? window.requestAnimationFrame(frame) : 0;
      if (!raf) last = 0;
    };

    const wake = () => {
      if (reduce.matches) {
        paint(Infinity);
        return;
      }
      if (!raf) raf = window.requestAnimationFrame(frame);
    };

    const start = () => {
      window.cancelAnimationFrame(raf);
      raf = 0;
      last = 0;
      size();
      readColours();
      sewn.x = target();
      sewn.v = 0;
      wake();
    };

    const onResize = () => {
      size();
      wake();
    };
    const relayout = new ResizeObserver(wake);
    const theme = new MutationObserver(() => {
      readColours();
      if (reduce.matches) paint(Infinity);
      else paint(sewn.x);
    });

    start();
    reduce.addEventListener("change", start);
    window.addEventListener("scroll", wake, { passive: true });
    window.addEventListener("resize", onResize);
    relayout.observe(page);
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ["data-mode"] });
    return () => {
      window.cancelAnimationFrame(raf);
      reduce.removeEventListener("change", start);
      window.removeEventListener("scroll", wake);
      window.removeEventListener("resize", onResize);
      relayout.disconnect();
      theme.disconnect();
    };
  }, []);

  return <canvas ref={ref} aria-hidden className={styles.thread} data-slot="pixel-thread" />;
}
