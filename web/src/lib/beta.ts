/**
 * A request to join the private beta, as the landing page's form sends it to `/api/beta`. Pure, so the
 * route and the form share one definition and every case is a unit test.
 */
export const BETA_ROLES = ["own", "clients", "desk", "builder"] as const;
export type BetaRole = (typeof BETA_ROLES)[number];

export type BetaRequest = { email: string; role: BetaRole | null };

export type BetaParse = { ok: true; request: BetaRequest } | { ok: false; reason: "email" | "bot" };

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const EMAIL_MAX = 254;

function isRole(value: unknown): value is BetaRole {
  return typeof value === "string" && (BETA_ROLES as readonly string[]).includes(value);
}

/**
 * The form's body, checked. `website` is a field people never see: a value in it means a bot filled
 * the form, which the route answers as if it had worked and stores nothing.
 */
export function parseBetaRequest(body: unknown): BetaParse {
  if (typeof body !== "object" || body === null) return { ok: false, reason: "email" };
  const { email, role, website } = body as Record<string, unknown>;
  if (typeof website === "string" && website.trim() !== "") return { ok: false, reason: "bot" };
  if (typeof email !== "string") return { ok: false, reason: "email" };
  const address = email.trim().toLowerCase();
  if (address.length > EMAIL_MAX || !EMAIL.test(address)) return { ok: false, reason: "email" };
  return { ok: true, request: { email: address, role: isRole(role) ? role : null } };
}
