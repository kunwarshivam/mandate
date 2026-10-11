import { DEMO_PATH } from "@/lib/auth-routes";

export { DEMO_PATH };

/**
 * What the landing page's browser and the example app in its tab say to each other (DEC-906), by
 * `postMessage` between two documents of the same origin: the app reports where it is and whether it
 * can go back or forward; the browser asks where it is, or asks it to go back, forward, to an address,
 * or to start over. Each side accepts a message only from the other window and from this origin.
 */
export const DEMO_SOURCE = "owlhead-demo";

export type DemoReport = {
  source: typeof DEMO_SOURCE;
  type: "at";
  path: string;
  back: boolean;
  forward: boolean;
};

/** What the browser asks of the app. Asking where it is answers with a report, for a browser that started listening after the first one. */
export type DemoAsk = { type: "where" } | { type: "go"; to: "back" | "forward" | "restart" } | { type: "open"; path: string };

export type DemoCommand = DemoAsk & { source: typeof DEMO_SOURCE };

function fromDemo(data: unknown): data is { source: typeof DEMO_SOURCE; type: unknown } {
  return typeof data === "object" && data !== null && (data as { source?: unknown }).source === DEMO_SOURCE;
}

export function isDemoReport(data: unknown): data is DemoReport {
  if (!fromDemo(data) || data.type !== "at") return false;
  const d = data as Partial<DemoReport>;
  return typeof d.path === "string" && d.path.startsWith("/") && typeof d.back === "boolean" && typeof d.forward === "boolean";
}

export function isDemoCommand(data: unknown): data is DemoCommand {
  if (!fromDemo(data)) return false;
  const d = data as { type: unknown; to?: unknown; path?: unknown };
  if (d.type === "where") return true;
  if (d.type === "go") return d.to === "back" || d.to === "forward" || d.to === "restart";
  return d.type === "open" && typeof d.path === "string" && d.path.startsWith("/") && !d.path.startsWith("//");
}

/** An address typed into the browser, as a path in the app: `app.owlhead.ai/agents`, `https://app.owlhead.ai/agents` and `/agents` are all `/agents`. */
export function addressPath(typed: string): string | null {
  const t = typed.trim();
  if (t === "") return null;
  const bare = t.replace(/^https?:\/\//i, "");
  const host = /^app\.owlhead\.ai(?=\/|$)/i.exec(bare);
  const path = host ? bare.slice(host[0].length) || "/" : bare;
  if (!path.startsWith("/") || path.startsWith("//") || /[\\\s\u0000-\u001f\u007f]/.test(path)) return null;
  return path;
}
