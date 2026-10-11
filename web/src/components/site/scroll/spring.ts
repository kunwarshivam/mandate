/**
 * Damped springs for the long page (DEC-907): one stepped each frame for the flying owl, and the
 * same physics sampled into CSS `linear()` curves for the pieces that settle as they come into view.
 */

/** A value on a spring, and how fast it is moving, in units per second. */
export interface Spring {
  x: number;
  v: number;
}

export interface SpringParams {
  /** Pull towards the target per unit of distance, per second squared. */
  stiffness: number;
  /** Drag per unit of speed, per second; under 2√stiffness it overshoots. */
  damping: number;
}

/** The longest step the integrator takes at once, so a slow frame never makes a spring blow up. */
const MAX_STEP = 1 / 120;

/** Moves a spring `dt` seconds towards `target`, by semi-implicit Euler in steps of at most `MAX_STEP`. */
export function stepSpring(s: Spring, target: number, dt: number, { stiffness, damping }: SpringParams): void {
  let left = Math.min(Math.max(dt, 0), 0.25);
  while (left > 0) {
    const h = Math.min(left, MAX_STEP);
    s.v += (stiffness * (target - s.x) - damping * s.v) * h;
    s.x += s.v * h;
    left -= h;
  }
}

/** The unit step of a spring with damping ratio `zeta` (0 < zeta < 1), at time `t` in periods. */
export function springAt(zeta: number, t: number): number {
  const w = 2 * Math.PI;
  const wd = w * Math.sqrt(1 - zeta * zeta);
  return 1 - Math.exp(-zeta * w * t) * (Math.cos(wd * t) + ((zeta * w) / wd) * Math.sin(wd * t));
}

/** Periods until the spring stays within 0.4% of rest. */
export function settleTime(zeta: number): number {
  return Math.log(0.004 * Math.sqrt(1 - zeta * zeta)) / (-zeta * 2 * Math.PI);
}

/** A spring with damping ratio `zeta` as a CSS `linear()` easing, from rest to rest, in `points` samples. */
export function springEasing(zeta: number, points = 48): string {
  const end = settleTime(zeta);
  const stops = Array.from({ length: points + 1 }, (_, i) => (i === points ? 1 : Math.round(springAt(zeta, (i / points) * end) * 1000) / 1000));
  return `linear(${stops.join(", ")})`;
}

/** The page's three springs, as CSS custom properties: a soft settle, a firmer one, and a bounce. */
export const SPRING_VARS = {
  "--spring-soft": springEasing(0.7),
  "--spring": springEasing(0.55),
  "--spring-bounce": springEasing(0.38),
} as const;
