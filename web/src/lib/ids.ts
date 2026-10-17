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
