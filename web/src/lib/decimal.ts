/**
 * Fixed-point arithmetic on the schema's decimal strings (`$defs/decimal`). Money and quantities
 * never pass through binary floating point: values are scaled integers with 12 places, the
 * precision the mandate spec rounds ratios to (§5.2).
 */

const SCALE = 12;
const FACTOR = 10n ** BigInt(SCALE);
const DECIMAL = /^-?(0|[1-9][0-9]*)(\.[0-9]+)?$/;

export type Dec = bigint;

export function dec(value: string): Dec {
  if (!DECIMAL.test(value)) throw new Error(`not a decimal string: ${value}`);
  const negative = value.startsWith("-");
  const body = negative ? value.slice(1) : value;
  const [whole, fraction = ""] = body.split(".");
  if (fraction.length > SCALE) throw new Error(`more than ${SCALE} places: ${value}`);
  const scaled = BigInt(whole) * FACTOR + BigInt(fraction.padEnd(SCALE, "0"));
  return negative ? -scaled : scaled;
}

export const add = (...values: Dec[]): Dec => values.reduce((sum, v) => sum + v, 0n);
export const sub = (a: Dec, b: Dec): Dec => a - b;
export const neg = (a: Dec): Dec => -a;
export const abs = (a: Dec): Dec => (a < 0n ? -a : a);
export const min = (a: Dec, b: Dec): Dec => (a < b ? a : b);
export const max = (a: Dec, b: Dec): Dec => (a > b ? a : b);
export const ONE = FACTOR;
export const ZERO = 0n;

/** Product of two scaled values, truncated toward zero at 12 places. */
export function mul(a: Dec, b: Dec): Dec {
  return (a * b) / FACTOR;
}

/** Quotient of two scaled values, truncated toward zero at 12 places. */
export function div(a: Dec, b: Dec): Dec {
  if (b === 0n) throw new Error("division by zero");
  return (a * FACTOR) / b;
}

export function fromInt(n: number): Dec {
  if (!Number.isInteger(n)) throw new Error(`not an integer: ${n}`);
  return BigInt(n) * FACTOR;
}

/** Rounds half away from zero to `places` and returns the plain decimal string. */
export function toFixed(value: Dec, places: number): string {
  const negative = value < 0n;
  const magnitude = negative ? -value : value;
  const unit = 10n ** BigInt(SCALE - places);
  const rounded = (magnitude + unit / 2n) / unit;
  const digits = rounded.toString().padStart(places + 1, "0");
  const whole = digits.slice(0, digits.length - places);
  const fraction = places > 0 ? `.${digits.slice(digits.length - places)}` : "";
  const isZero = rounded === 0n;
  return `${negative && !isZero ? "-" : ""}${whole}${fraction}`;
}

/** The schema's canonical form: no trailing zeros, no trailing point. */
export function toDecimalString(value: Dec): string {
  const fixed = toFixed(value, SCALE);
  return fixed.includes(".") ? fixed.replace(/0+$/, "").replace(/\.$/, "") : fixed;
}

export function sign(value: Dec): -1 | 0 | 1 {
  return value < 0n ? -1 : value > 0n ? 1 : 0;
}

/** value / whole as a fraction in [0, 1], for drawing only; never feeds back into money. */
export function ratio(value: Dec, whole: Dec): number {
  if (whole <= 0n) return 0;
  const clamped = value < 0n ? 0n : value > whole ? whole : value;
  return Number((clamped * 10_000n) / whole) / 10_000;
}
