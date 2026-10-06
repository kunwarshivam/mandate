import { sha256Bytes } from "./sha256";

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** The fixtures' IDs all start inside the same ULID millisecond range, so new ones sort with them. */
const TIME_PREFIX = "01JC";

/**
 * A ULID-shaped ID (26 Crockford base-32 characters) drawn from `seed`, so the same seed always
 * names the same thing. The fixture runtime seeds it with what it is naming and a counter.
 */
export function fixtureUlid(seed: string): string {
  const bytes = sha256Bytes(seed);
  let bits = 0;
  let buffer = 0;
  let out = TIME_PREFIX;
  for (const byte of bytes) {
    buffer = (buffer << 8) | byte;
    bits += 8;
    while (bits >= 5 && out.length < 26) {
      bits -= 5;
      out += CROCKFORD[(buffer >>> bits) & 31];
    }
    buffer &= (1 << bits) - 1;
    if (out.length === 26) break;
  }
  return out;
}

/** A UUID-shaped asset ID for an instrument the fixtures do not list. */
export function fixtureAssetId(symbol: string): string {
  const hex = Array.from(sha256Bytes(`asset:${symbol}`), (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-4${hex.slice(13, 16)}-9${hex.slice(17, 20)}-${hex.slice(20, 32)}`;
}
