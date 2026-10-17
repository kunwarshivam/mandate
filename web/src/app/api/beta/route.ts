import { SUPABASE_PUBLISHABLE_KEY, SUPABASE_URL } from "@/lib/auth-config";
import { betaOpen, parseBetaRequest } from "@/lib/beta";
import { storeBetaRequest } from "@/lib/beta-store";

const NO_STORE = { "Cache-Control": "no-store" };

/** The private beta's request form (`web/src/components/site/beta-form.tsx`). Closed, it reads nothing and stores nothing (DEC-987). */
export async function POST(request: Request) {
  if (!betaOpen(process.env.NODE_ENV, SUPABASE_URL, SUPABASE_PUBLISHABLE_KEY, process.env.OWLHEAD_E2E_SCENARIOS)) {
    return Response.json({ error: "closed" }, { status: 403, headers: NO_STORE });
  }
  let body: unknown;
  try {
    body = await request.json();
  } catch {
    return Response.json({ error: "email" }, { status: 400, headers: NO_STORE });
  }
  const parsed = parseBetaRequest(body);
  if (!parsed.ok) {
    return parsed.reason === "bot" ? Response.json({ ok: true }, { headers: NO_STORE }) : Response.json({ error: "email" }, { status: 400, headers: NO_STORE });
  }
  const stored = await storeBetaRequest(parsed.request);
  return stored ? Response.json({ ok: true }, { headers: NO_STORE }) : Response.json({ error: "unavailable" }, { status: 503, headers: NO_STORE });
}
