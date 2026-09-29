import { parseBetaRequest } from "@/lib/beta";
import { storeBetaRequest } from "@/lib/beta-store";

const NO_STORE = { "Cache-Control": "no-store" };

/** The private beta's request form (`web/src/components/site/beta-form.tsx`). */
export async function POST(request: Request) {
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
