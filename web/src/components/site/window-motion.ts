/**
 * How the desktop's windows move, as a Mac's do: a window zooms out of whatever opened it, scales
 * down into its taskbar button when minimized (the Dock's Scale effect), shrinks a little and fades
 * as it closes, and morphs its frame when maximized or restored. Only transform and opacity animate.
 * Every motion starts from where the window is on screen, so a second click mid-flight turns it
 * around rather than jumping. Reduced motion keeps the fades and drops the movement, and a browser
 * without the Web Animations API (jsdom) changes state at once.
 */

type Box = Pick<DOMRect, "left" | "top" | "width" | "height">;

type Pose = { transform: string; opacity: string };

/** A window opened or closed with nothing to zoom from starts or ends this much smaller, never from nothing. */
const NEAR = 0.94;

const FALLBACK = { ease: "cubic-bezier(0.23, 1, 0.32, 1)", travel: "cubic-bezier(0.77, 0, 0.175, 1)", open: 280, exit: 220, close: 150, frame: 240 };

function token(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** A duration token in milliseconds, whichever unit the stylesheet ended up writing it in (`280ms`, `.28s`). */
export function millis(value: string): number | null {
  const m = /^(\d*\.?\d+)(ms|s)$/.exec(value.trim());
  if (!m) return null;
  return Number(m[1]) * (m[2] === "s" ? 1000 : 1);
}

function timing() {
  const ms = (name: string, fallback: number) => millis(token(name)) ?? fallback;
  return {
    ease: token("--ease-out") || FALLBACK.ease,
    travel: token("--ease-in-out") || FALLBACK.travel,
    open: ms("--duration-window", FALLBACK.open),
    exit: ms("--duration-window-exit", FALLBACK.exit),
    close: ms("--duration-dialog-exit", FALLBACK.close),
    frame: ms("--duration-dialog", FALLBACK.frame),
  };
}

const animates = (el: HTMLElement) => typeof el.animate === "function";

const still = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

const LEAVING = "leaving";

/** Whether a window is between two states, so a click on it now should turn it around. */
export function moving(el: HTMLElement | null): boolean {
  return !!el && animates(el) && el.getAnimations().length > 0;
}

/** Whether a window is on its way out, so its taskbar button should bring it back rather than send it again. */
export function leaving(el: HTMLElement | null): boolean {
  return !!el && moving(el) && el.getAnimations().some((a) => a.id === LEAVING);
}

/** The box a window would have with no motion applied, and the pose it is showing right now. */
function halt(el: HTMLElement): { box: DOMRect; now: Pose | null } {
  if (!moving(el)) return { box: el.getBoundingClientRect(), now: null };
  const style = getComputedStyle(el);
  const now = { transform: style.transform, opacity: style.opacity };
  for (const a of el.getAnimations()) a.cancel();
  return { box: el.getBoundingClientRect(), now };
}

function shrunk(box: Box, by: number): Box {
  return { left: box.left + (box.width * (1 - by)) / 2, top: box.top + (box.height * (1 - by)) / 2, width: box.width * by, height: box.height * by };
}

/**
 * The transform that lays a window over `to`, keeping its proportions and centred on it, from a
 * top-left transform origin (the window's `origin-top-left`).
 */
export function onto(box: Box, to: Box): string {
  const s = Math.min(to.width / box.width, to.height / box.height);
  const dx = to.left + to.width / 2 - (box.left + (box.width * s) / 2);
  const dy = to.top + to.height / 2 - (box.top + (box.height * s) / 2);
  return `translate(${dx}px, ${dy}px) scale(${s})`;
}

/** A source worth zooming from: on screen and not collapsed to nothing. */
function usable(from: Box | null): from is Box {
  return !!from && from.width > 0 && from.height > 0;
}

/** Zooms a window that has just been shown out of `from`, the box of whatever opened it. */
export function openFrom(el: HTMLElement, from: Box | null): void {
  if (!animates(el)) return;
  const { box, now } = halt(el);
  const t = timing();
  const start: Pose = now ?? { transform: still() ? "none" : onto(box, usable(from) ? from : shrunk(box, NEAR)), opacity: "0" };
  el.animate([start, { transform: "none", opacity: "1" }], { duration: still() ? t.exit : t.open, easing: t.ease });
}

/**
 * Sends a window into `to` (its taskbar button) or, with nowhere to go, shrinks and fades it, then
 * calls `done`, which must hide it synchronously so the window never shows a frame at full size.
 * Interrupted by another motion, it never calls `done`.
 */
export function leave(el: HTMLElement, to: Box | null, done: () => void): void {
  if (!animates(el)) return done();
  const { box, now } = halt(el);
  const t = timing();
  const end = still() ? "none" : onto(box, usable(to) ? to : shrunk(box, NEAR));
  const motion = el.animate([now ?? { transform: "none", opacity: "1" }, { transform: end, opacity: "0" }], {
    id: LEAVING,
    duration: usable(to) ? t.exit : t.close,
    easing: usable(to) ? t.travel : t.ease,
    fill: "forwards",
  });
  motion.finished.then(
    () => {
      done();
      motion.cancel();
    },
    () => {},
  );
}

/** How far a dragged window leans, in degrees per pixel per millisecond of sideways speed, and at most. */
const LEAN = 1.6;
const MAX_LEAN = 2.5;

/** A window let go faster than this, in pixels per millisecond, glides on; how long it glides for, and at most how far. */
const THROWN = 0.35;
const GLIDE = 140;
const MAX_GLIDE = 220;

/** Whether a window should lean and glide as it is dragged: only with the Web Animations API and without reduced motion. */
export function physical(el: HTMLElement): boolean {
  return animates(el) && !still();
}

/** The lean for a sideways speed, in pixels per millisecond: into the direction of travel, as a card dragged across a table. */
export function leanFor(speed: number): number {
  return Math.max(-MAX_LEAN, Math.min(MAX_LEAN, speed * LEAN));
}

/** How much further a window let go at `speed` (pixels per millisecond, each axis) glides. */
export function glideFor(speed: { x: number; y: number }): { x: number; y: number } {
  const fast = Math.hypot(speed.x, speed.y);
  if (fast < THROWN) return { x: 0, y: 0 };
  const scale = Math.min(GLIDE, MAX_GLIDE / fast);
  return { x: speed.x * scale, y: speed.y * scale };
}

/**
 * Settles a window just let go: it glides the last `glide` pixels into the place it has already
 * been moved to, decelerating, and its lean springs back to level. The lean was turned about the
 * point it was held by, which is put back once it is level.
 */
export function settle(el: HTMLElement, glide: { x: number; y: number }, lean: number): void {
  const held = el.style.transformOrigin;
  el.style.rotate = "";
  el.style.transition = "";
  if (!physical(el)) {
    el.style.transformOrigin = "";
    return;
  }
  const t = timing();
  const motions = [el.animate([{ rotate: `${lean}deg` }, { rotate: "0deg" }], { duration: 650, easing: token("--ease-spring") || t.ease })];
  if (glide.x !== 0 || glide.y !== 0) motions.push(el.animate([{ transform: `translate(${-glide.x}px, ${-glide.y}px)` }, { transform: "none" }], { duration: 480, easing: t.ease }));
  Promise.allSettled(motions.map((m) => m.finished)).then(() => {
    if (el.style.transformOrigin === held) el.style.transformOrigin = "";
  });
}

/** Morphs a window from the frame it has to the one `change` gives it (maximize and restore). */
export function reframe(el: HTMLElement, change: () => void): void {
  if (!animates(el) || still()) return change();
  const first = el.getBoundingClientRect();
  halt(el);
  change();
  const last = el.getBoundingClientRect();
  if (last.width === 0 || last.height === 0) return;
  const t = timing();
  const from = `translate(${first.left - last.left}px, ${first.top - last.top}px) scale(${first.width / last.width}, ${first.height / last.height})`;
  el.animate([{ transform: from }, { transform: "none" }], { duration: t.frame, easing: t.ease });
}
