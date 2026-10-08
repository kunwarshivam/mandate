import type { NextRequest, NextResponse } from "next/server";

/**
 * The edge entry point that replaces `src/proxy.ts` on Cloudflare Workers (DEC-823, DEC-731): OpenNext
 * runs only edge middleware, not Node's proxy. The implementation moves this file to `src/middleware.ts`
 * and removes the proxy; Next refuses a build that has both.
 */
export async function middleware(request: NextRequest): Promise<NextResponse> {
  void request;
  throw new Error("Unimplemented: E11-9");
}

export const config = {
  matcher: ["/((?!_next/|art/|video/|favicon|apple-touch-icon\\.png|pwa-|og-image\\.png|site\\.webmanifest|robots\\.txt).*)"],
};
