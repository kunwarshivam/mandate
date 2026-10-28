/**
 * DEC-2411's build flag, `NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED`: read here from the value it is given,
 * so `next.config.ts` can refuse a production build that carries it and `@/api/passkey` can read it,
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
 * Refuses a production build that would turn the flag on, whatever set it (a shell, Workers Builds
 * or GitHub variables, a `.env` file): `next build` inlines `NEXT_PUBLIC_*` into the client bundle,
 * so a deployed build must never be made with it (DEC-2411 item 2).
 */
export function refuseProductionWaiver(nodeEnv: string | undefined, value: string | undefined): void {
  if (nodeEnv === "production" && coolOffWaivedFrom(value)) throw new Error("NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED is a test cell's flag for next dev; a production build refuses it (DEC-2411)");
}
