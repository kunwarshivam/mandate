/** Opaque route IDs: a type prefix and a ULID (Crockford base32, 26 characters). */
const ULID = "[0-9A-HJKMNP-TV-Z]{26}";

export const AGENT_ID = new RegExp(`^agt_${ULID}$`);
export const APPROVAL_ID = new RegExp(`^apr_${ULID}$`);
export const ORDER_ID = new RegExp(`^cid_${ULID}$`);
export const EVENT_ID = new RegExp(`^${ULID}$`);
/** Broker asset IDs are UUIDs; routes use them so an instrument's ticker stays out of history. */
export const ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
export const CONNECTION_ID = new RegExp(`^con_${ULID}$`);
/** A backtest is named by the API's opaque `Id` (`common.schema.json`), which embeds nothing. */
export const BACKTEST_ID = /^[A-Za-z0-9_-]{1,64}$/;

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/**
 * A new agent's id, `agt_` and a ULID: 48 bits of the time in milliseconds, then 80 random bits from
 * the browser's generator. The deploy names it (spec §3.11 item 11), and an agent id is deployed
 * once (DEC-1173 item 1), so it is minted once per set-up and kept across that set-up's retries.
 */
export function newAgentId(nowMs: number, random: (bytes: Uint8Array) => Uint8Array = (b) => crypto.getRandomValues(b)): string {
  let time = "";
  if (!Number.isSafeInteger(Math.floor(nowMs)) || nowMs < 0) throw new RangeError("an agent id needs the time in milliseconds");
  let t = Math.floor(nowMs);
  for (let i = 0; i < 10; i += 1) {
    time = CROCKFORD[t % 32] + time;
    t = Math.floor(t / 32);
  }
  const bytes = random(new Uint8Array(10));
  let bits = 0n;
  for (const b of bytes) bits = (bits << 8n) | BigInt(b);
  let rest = "";
  for (let i = 15; i >= 0; i -= 1) rest += CROCKFORD[Number((bits >> BigInt(i * 5)) & 31n)];
  return `agt_${time}${rest}`;
}
