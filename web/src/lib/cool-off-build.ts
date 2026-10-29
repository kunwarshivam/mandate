/**
 * DEC-2411's build flag, `NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED`: read here from the value it is given,
 * so `next.config.ts` can refuse any build but `next dev` that carries it and `@/api/passkey` can read it,
 * neither of them depending on the other. Imports nothing, as `next.config.ts` loads it.
 */

/**
 * Whether a build lets a passkey in its cool-off be tried: only `"1"`, which only the local cell's
 * Playwright config sets, on `next dev`, when the cell waives a first passkey's cool-off (DEC-2146).
 * It drops the browser's own pre-check and nothing else; the server stays the only judge.
 */
export function coolOffWaivedFrom(value: string | undefined): boolean {
  return value === "1";
}

/**
 * Refuses any build but `next dev` (`NODE_ENV=development`) that would turn the flag on, whatever set
 * it (a shell, Workers Builds or GitHub variables, a `.env` file): `next build` inlines
 * `NEXT_PUBLIC_*` into the client bundle under any `NODE_ENV`, so only the cell's `next dev` may be
 * made with it (DEC-2411 item 2). An allow-list of one, so an unforeseen environment refuses.
 */
export function refuseWaiverOutsideDev(nodeEnv: string | undefined, value: string | undefined): void {
  if (nodeEnv !== "development" && coolOffWaivedFrom(value)) throw new Error(`NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED is a test cell's flag for next dev; a build with NODE_ENV=${nodeEnv ?? "unset"} refuses it (DEC-2411)`);
}

/**
 * The staging demo's waiver in the browser (DEC-2150 on the server; this file's second flag,
 * `NEXT_PUBLIC_OWLHEAD_STAGING_COOL_OFF_UNTIL`): until this instant, the browser tries a passkey still
 * in its cool-off and leaves the verdict to the server, which waives it on staging until its own
 * expiry. It can be no later than the server's compiled ceiling, so a build cannot outlive it; past
 * the instant it is inert. It drops the browser's pre-check and nothing else.
 */
export const STAGING_WAIVER_CEILING = "2026-11-05T00:00:00Z";
const UNTIL = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/;

/** The instant, in milliseconds, a well-formed value no later than the ceiling names; `null` for unset. Throws for anything else. */
export function stagingWaiverUntil(value: string | undefined): number | null {
  if (value === undefined || value === "") return null;
  const at = UNTIL.test(value) ? Date.parse(value) : Number.NaN;
  if (Number.isNaN(at) || new Date(at).toISOString().replace(".000Z", "Z") !== value) throw new Error(`NEXT_PUBLIC_OWLHEAD_STAGING_COOL_OFF_UNTIL must be YYYY-MM-DDTHH:MM:SSZ in UTC, not ${JSON.stringify(value)}`);
  if (at > Date.parse(STAGING_WAIVER_CEILING)) throw new Error(`NEXT_PUBLIC_OWLHEAD_STAGING_COOL_OFF_UNTIL ${value} is later than the ceiling ${STAGING_WAIVER_CEILING} (DEC-2150 item 3)`);
  return at;
}

/** Whether the staging waiver holds at `nowMs`: only strictly before its instant, as the server's expiry ends it. */
export function stagingWaived(until: number | null, nowMs: number): boolean {
  return until !== null && nowMs < until;
}
