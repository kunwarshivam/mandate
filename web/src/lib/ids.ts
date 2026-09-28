/** Opaque route IDs: a type prefix and a ULID (Crockford base32, 26 characters). */
const ULID = "[0-9A-HJKMNP-TV-Z]{26}";

export const AGENT_ID = new RegExp(`^agt_${ULID}$`);
export const APPROVAL_ID = new RegExp(`^apr_${ULID}$`);
