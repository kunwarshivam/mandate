"use client";

import { useEffect, useRef } from "react";
import { type Spring, type SpringParams, stepSpring } from "./spring";
import { OWL_HEIGHT, type OwlInks, type Pose, type VoxelInk, drawOwl, owlVoxels, skin } from "./voxel-owl";
import styles from "./scroll.module.css";

/** From the owl's centre down to its feet, in owl heights: the talons' lower edge is half the sprite below the middle. */
const FEET = 0.5;
/** The canvas is wider than the owl, so lifted wings and a leaning owl are never cut off. */
const ROOM = 2.1;
/** Wing beats a second, and how high a beat lifts the wings at full speed, in radians. */
const BEATS = 3.6;
const LIFT = 1.75;
/** At rest the owl looks over its shoulder this often, for this long, in milliseconds. */
const GLANCE_EVERY = 6800;
const GLANCE_FOR = 1500;
/** The long page's bar, which the owl never flies under. */
const BAR = 56;

const TRAVEL: SpringParams = { stiffness: 140, damping: 15 };
const HOP: SpringParams = { stiffness: 60, damping: 9 };
const TURN: SpringParams = { stiffness: 90, damping: 11 };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The owl's height in CSS pixels at a viewport width. */
export function owlSize(width: number): number {
  return width < 640 ? 66 : 100;
}

/** The logo's ink by default; pale, in paper feathers, where it stands on tide and ink would sink into it. */
export type Coat = "ink" | "pale";

const INKS: VoxelInk[] = ["line", "feathers", "trim", "disc", "iris", "pupil", "glint", "beak"];

/** The owl's colours, read from `scroll.module.css`, which points each at a token for the theme and the coat. */
function inksOf(canvas: HTMLElement, coat: Coat): OwlInks {
  if (coat === "pale") canvas.dataset.coat = "pale";
  else delete canvas.dataset.coat;
  const css = getComputedStyle(canvas);
  return Object.fromEntries(INKS.map((ink) => [ink, css.getPropertyValue(`--owl-${ink}`).trim()])) as OwlInks;
}

export interface Perch {
  x: number;
  y: number;
  yaw: number;
  coat: Coat;
}

/** Where the owl stands on an element marked `data-perch`: on its top edge, `data-perch-at` of the way across. */
export function perchOn(el: HTMLElement, owl: number, origin = { left: 0, top: 0 }): Perch {
  const r = el.getBoundingClientRect();
  const at = Number(el.dataset.perchAt ?? 0.5);
  return { x: r.left - origin.left + r.width * at, y: r.top - origin.top - owl * FEET, yaw: Number(el.dataset.perchYaw ?? 0), coat: el.dataset.perchCoat === "pale" ? "pale" : "ink" };
}

/** The perch whose top is nearest two fifths of the way down the window, keeping the current one unless another is clearly nearer. */
export function nearestPerch(perches: HTMLElement[], height: number, current: HTMLElement | null): HTMLElement | null {
  let best: HTMLElement | null = null;
  let score = Infinity;
  for (const el of perches) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    const off = r.bottom < 0 || r.top > height ? height : 0;
    const s = Math.abs(r.top - height * 0.42) + off - (el === current ? 60 : 0);
    if (s < score) {
      score = s;
      best = el;
    }
  }
  return best;
}

/**
 * The brand owl in three dimensions, flying down the long page (DEC-907). It peeks over the scroll
 * cue on the first screen, rides the top edge of the page as it rises, then hops from perch to perch
 * on springs: it leans into its travel, stretches when it moves fast, turns its head to the pointer,
 * bobs when it rests, and blinks. With motion reduced it stands still on the first perch.
 */
export function OwlFlight() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    const page = canvas?.closest<HTMLElement>("[data-slot=long-page]");
    const g = canvas?.getContext("2d");
    if (!canvas || !page || !g) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
    const open = skin(owlVoxels(false));
    const shut = skin(owlVoxels(true));
    let coat: Coat = "ink";
    let inks = inksOf(canvas, coat);
    const wear = (next: Coat) => {
      if (next === coat) return;
      coat = next;
      inks = inksOf(canvas, coat);
    };
    let owl = owlSize(window.innerWidth);
    let box = Math.round(owl * ROOM);
    let raf = 0;

    const size = () => {
      owl = owlSize(window.innerWidth);
      box = Math.round(owl * ROOM);
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = box * dpr;
      canvas.height = box * dpr;
      canvas.style.width = `${box}px`;
      canvas.style.height = `${box}px`;
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
    };

    const paint = (pose: Pose, clipTop = Infinity) => {
      g.clearRect(0, 0, box, box);
      g.save();
      if (clipTop < box) {
        g.beginPath();
        g.rect(0, 0, box, Math.max(0, clipTop));
        g.clip();
      }
      drawOwl(g, pose.blink ? shut : open, pose, inks, box / 2, box / 2, owl / OWL_HEIGHT);
      g.restore();
    };

    const perches = () => [...page.querySelectorAll<HTMLElement>("[data-perch]")];

    const rest = () => {
      const first = perches()[0];
      canvas.style.position = "absolute";
      if (!first) return;
      const p = perchOn(first, owl, page.getBoundingClientRect());
      wear(p.coat);
      canvas.style.transform = `translate(${p.x - box / 2}px, ${p.y - box / 2}px)`;
      paint({ yaw: p.yaw, pitch: 0.18, roll: 0, head: -p.yaw * 0.6, flap: 0, stretch: 1, blink: false });
      canvas.dataset.owl = "resting";
    };

    const x: Spring = { x: window.innerWidth / 2, v: 0 };
    const y: Spring = { x: window.innerHeight + owl, v: 0 };
    const hop: Spring = { x: 0, v: 0 };
    const yaw: Spring = { x: 0, v: 0 };
    const pitch: Spring = { x: 0.18, v: 0 };
    const roll: Spring = { x: 0, v: 0 };
    const head: Spring = { x: 0, v: 0 };
    let beat = 0;
    let lift = 0;
    let pointer: { x: number; y: number; at: number } | null = null;
    let current: HTMLElement | null = null;
    let last = 0;

    const frame = (now: number) => {
      const dt = last ? (now - last) / 1000 : 0;
      last = now;
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      const sheet = page.getBoundingClientRect().top;
      const cue = document.querySelector<HTMLElement>("[data-slot=scroll-cue][data-at]");
      const cueBox = cue?.getBoundingClientRect();
      const list = perches();
      let tx: number;
      let ty: number;
      let tyaw = 0;
      let up = 0;
      let clip = Infinity;
      let tcoat: Coat = "ink";
      const peeking = !!cueBox && cueBox.width > 0 && vw >= 1024 && sheet >= vh - 2;
      if (peeking && cueBox) {
        tx = cueBox.left + cueBox.width / 2;
        ty = cueBox.top - owl * 0.02 + Math.sin(now / 700) * owl * 0.06;
        tyaw = Math.sin(now / 1100) * 0.7;
        clip = cueBox.top;
      } else if (sheet > vh * 0.4) {
        const from = cueBox && cueBox.width > 0 ? cueBox.left + cueBox.width / 2 : vw / 2;
        const to = list[0] ? perchOn(list[0], owl).x : vw * 0.8;
        const risen = clamp((vh - sheet) / (vh * 0.6), 0, 1);
        tx = from + (to - from) * risen;
        ty = vw >= 1024 || sheet < vh - owl * 1.2 ? sheet - owl * FEET : vh + owl;
        if (cueBox && cueBox.width > 0 && sheet > cueBox.top) clip = cueBox.top;
      } else {
        current = nearestPerch(list, vh, current);
        const p: Perch = current ? perchOn(current, owl) : { x: vw * 0.8, y: vh * 0.5, yaw: 0, coat: "ink" };
        tcoat = p.coat;
        tx = clamp(p.x, owl * 0.6, vw - owl * 0.6);
        ty = clamp(p.y, BAR + owl * 0.55, vh - owl * 0.6);
        tyaw = p.yaw;
        up = -Math.min(owl * 1.2, Math.hypot(tx - x.x, ty - y.x) * 0.35);
      }
      wear(tcoat);
      stepSpring(x, tx, dt, TRAVEL);
      stepSpring(y, ty, dt, TRAVEL);
      stepSpring(hop, up, dt, HOP);
      const vx = x.v;
      const vy = y.v + hop.v;
      const speed = Math.hypot(vx, vy);
      const watching = pointer !== null && now - pointer.at < 2500;
      const glance = !peeking && speed < 40 && !watching && now % GLANCE_EVERY < GLANCE_FOR ? (Math.floor(now / GLANCE_EVERY) % 2 ? 1.1 : -1.1) : 0;
      const look = watching && pointer ? clamp(((pointer.x - x.x) / vw) * 2.6, -1.2, 1.2) : glance;
      stepSpring(yaw, tyaw, dt, TURN);
      stepSpring(head, look - yaw.x * 0.5, dt, TURN);
      stepSpring(pitch, 0.18 + clamp(vy / 2400, -0.3, 0.3) + (watching && pointer ? clamp(((pointer.y - y.x) / vh) * 0.6, -0.2, 0.2) : 0), dt, TURN);
      stepSpring(roll, clamp(-vx / 1400, -0.45, 0.45), dt, TURN);
      const effort = peeking ? 0 : clamp((speed - 30) / 420, 0, 1);
      lift += (effort - lift) * Math.min(1, dt * 6);
      beat = lift > 0.02 ? beat + dt * BEATS * Math.PI * 2 : 0;
      const flap = lift * LIFT * (0.5 - 0.5 * Math.cos(beat));
      const bob = peeking ? 0 : (1 - clamp(speed / 240, 0, 1)) * Math.sin(now / 520) * owl * 0.035;
      const top = y.x + hop.x + bob - box / 2;
      canvas.style.transform = `translate3d(${x.x - box / 2}px, ${top}px, 0)`;
      paint(
        { yaw: yaw.x, pitch: pitch.x, roll: roll.x, head: head.x, flap, stretch: 1 + clamp(Math.abs(vy) / 5000, 0, 0.14), blink: now % 4700 < 130 },
        clip - top,
      );
      canvas.dataset.owl = "flying";
      raf = window.requestAnimationFrame(frame);
    };

    const onPointer = (e: PointerEvent) => {
      if (e.pointerType === "mouse") pointer = { x: e.clientX, y: e.clientY, at: performance.now() };
    };
    const onResize = () => {
      size();
      if (reduce.matches) rest();
    };
    const theme = new MutationObserver(() => {
      inks = inksOf(canvas, coat);
      if (reduce.matches) rest();
    });

    const start = () => {
      window.cancelAnimationFrame(raf);
      size();
      if (reduce.matches) {
        rest();
        return;
      }
      canvas.style.position = "";
      last = 0;
      raf = window.requestAnimationFrame(frame);
    };

    start();
    reduce.addEventListener("change", start);
    window.addEventListener("resize", onResize);
    window.addEventListener("pointermove", onPointer, { passive: true });
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ["data-mode"] });
    return () => {
      window.cancelAnimationFrame(raf);
      reduce.removeEventListener("change", start);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("pointermove", onPointer);
      theme.disconnect();
    };
  }, []);

  return <canvas ref={ref} aria-hidden className={styles.owl} data-slot="flying-owl" />;
}
