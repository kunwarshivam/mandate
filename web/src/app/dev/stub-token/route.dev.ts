import { mintStubToken, readStubSignIn, sameOriginAsk } from "@/lib/stub-sign-in";

/**
 * `POST /dev/stub-token` (DEC-2406): the configured demo subject's token from the cell's stub issuer,
 * for the browser to trade at `POST /v1/sessions`. `next dev` only: a production build does not list
 * `dev.ts` as a page extension, so it has no such route, and this answers 404 whenever the stub
 * sign-in is off. The token is never logged and never cached.
 */
export const dynamic = "force-dynamic";

const NO_STORE = { "Cache-Control": "no-store" };

export async function POST(request: Request): Promise<Response> {
  const config = readStubSignIn(process.env, process.env.NODE_ENV);
  if (!config) return new Response(null, { status: 404, headers: NO_STORE });
  if (!sameOriginAsk(request.headers)) return new Response(null, { status: 403, headers: NO_STORE });
  const minted = await mintStubToken(config, fetch);
  if (!minted.ok) return new Response(null, { status: 502, headers: NO_STORE });
  return Response.json({ access_token: minted.accessToken }, { headers: NO_STORE });
}
