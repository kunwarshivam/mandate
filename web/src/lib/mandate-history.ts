import type { Agent, ContentRef, Iso, Mandate, MandateChange, MandateVersionRecord } from "@/fixtures/types";

type Value = MandateChange["from"];

/** RFC 6901: a pointer's reference tokens, unescaped. */
function tokens(pointer: string): string[] | null {
  if (!pointer.startsWith("/")) return null;
  return pointer
    .slice(1)
    .split("/")
    .map((t) => t.replaceAll("~1", "/").replaceAll("~0", "~"));
}

/**
 * Sets `pointer` in `doc` to `value` where it now holds `expected`, and says whether it did. A path
 * that does not resolve, or that holds anything else, means the history does not lead to `doc`.
 */
function undo(doc: unknown, pointer: string, expected: Value, value: Value): boolean {
  const keys = tokens(pointer);
  const last = keys?.pop();
  if (!keys || last === undefined) return false;
  let node: unknown = doc;
  for (const key of keys) {
    if (typeof node !== "object" || node === null || !Object.hasOwn(node, key)) return false;
    node = (node as Record<string, unknown>)[key];
  }
  if (typeof node !== "object" || node === null || !Object.hasOwn(node, last)) return false;
  const parent = node as Record<string, unknown>;
  if (parent[last] !== expected) return false;
  parent[last] = value;
  return true;
}

const applied = (v: MandateVersionRecord): boolean => v.application.result === "applied";

/**
 * The agent's mandate document as it stood at `version`: the current document with each applied
 * version's changes undone, newest first, along the chain of `previous` versions. `null` when the
 * version is not on that chain (never applied, another agent's, or unknown) or the history does not
 * lead back to it, so a caller shows what was recorded rather than the current rules.
 */
export function mandateAt(agent: Agent, version: ContentRef | undefined): Mandate | null {
  if (version === undefined) return null;
  if (version === agent.mandate_version) return agent.mandate;
  const doc = structuredClone(agent.mandate);
  let record = agent.versions.find((v) => v.mandate_version === agent.mandate_version && applied(v));
  for (let steps = 0; record && steps < agent.versions.length; steps += 1) {
    for (const c of [...record.changes].reverse()) if (!undo(doc, c.path, c.to, c.from)) return null;
    const previous = record.previous;
    if (previous === version) return doc;
    record = agent.versions.find((v) => v.mandate_version === previous && applied(v));
  }
  return null;
}

/** The version in force at `at`: the last one applied at or before it, or `null` before the first. */
export function versionInForce(versions: readonly MandateVersionRecord[], at: Iso): ContentRef | null {
  let inForce: { version: ContentRef; at: number } | null = null;
  for (const v of versions) {
    if (v.application.result !== "applied") continue;
    const appliedAt = Date.parse(v.application.at);
    if (appliedAt <= Date.parse(at) && (inForce === null || appliedAt >= inForce.at)) inForce = { version: v.mandate_version, at: appliedAt };
  }
  return inForce?.version ?? null;
}
