import { cookies } from "next/headers";
import { type MockServer, createMockServer } from "@/api/mock-server";
import { isScenario } from "@/fixtures/workspace";
import { SCENARIO_COOKIE, scenariosEnabled } from "@/lib/scenario";

/**
 * The fixture-backed mock of the workspace API at `/v1` (DEC-970), for `npm run dev` and the e2e build
 * only: with `OWLHEAD_DATA_SOURCE=api` and `OWLHEAD_API_ORIGIN` set to the app's own origin, the app
 * reads its workspace through the API's documented routes without a deployment. A production build
 * answers 404 to every path here (`scripts/no-scenarios.mjs` checks it): the app has no route of its own
 * in the API's place. The scenario cookie picks the fixture, and each scenario keeps its own commands.
 */
export const dynamic = "force-dynamic";

const servers = new Map<string, MockServer>();

async function answer(request: Request): Promise<Response> {
  if (!scenariosEnabled) return new Response(null, { status: 404 });
  const url = new URL(request.url);
  const workspace = url.pathname.split("/")[3] ?? "";
  const cookie = (await cookies()).get(SCENARIO_COOKIE)?.value;
  const scenario = isScenario(cookie) ? cookie : "normal";
  const key = `${scenario}\n${workspace}`;
  const server = servers.get(key) ?? createMockServer({ workspaceId: workspace, scenario });
  servers.set(key, server);
  try {
    return await server.fetch(`${url.pathname}${url.search}`, { method: request.method, headers: request.headers, body: request.method === "GET" ? undefined : await request.text() });
  } catch {
    return new Response(null, { status: 502 });
  }
}

export const GET = answer;
export const POST = answer;
