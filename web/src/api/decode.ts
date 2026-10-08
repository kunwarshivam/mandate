/**
 * Decoders for the workspace API's members (spec §3.1, §3.2). They never round: a JSON number where
 * a decimal belongs is refused, and a closed safety enum refuses a value it does not list, as a
 * typed issue the screen can render rather than an exception.
 */
import type { ContentRef, Decoder, Watermark } from "./types";

export const UNIMPLEMENTED = "Unimplemented: E11-9";

function unimplemented(): never {
  throw new Error(UNIMPLEMENTED);
}

/** A journal spec §4.6 canonical decimal string. */
export const decimal: Decoder<string> = () => unimplemented();

/** A journal spec §4.7 timestamp: UTC, nine fractional digits, `Z`. */
export const timestamp: Decoder<string> = () => unimplemented();

export const contentRef: Decoder<ContentRef> = () => unimplemented();

export const string: Decoder<string> = () => unimplemented();

/** A non-negative integer up to 2^53 − 1 (journal spec §4 item 4). */
export const integer: Decoder<number> = () => unimplemented();

export function closedEnum<const T extends string>(allowed: readonly T[]): Decoder<T> {
  void allowed;
  return () => unimplemented();
}

export function nullable<T>(inner: Decoder<T>): Decoder<T | null> {
  void inner;
  return () => unimplemented();
}

export function array<T>(item: Decoder<T>): Decoder<T[]> {
  void item;
  return () => unimplemented();
}

/** Reads the named members and ignores every other one (spec §3.2). */
export function object<T extends object>(members: { [K in keyof T]: Decoder<T[K]> }): Decoder<T> {
  void members;
  return () => unimplemented();
}

export const watermark: Decoder<Watermark> = () => unimplemented();
