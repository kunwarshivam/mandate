import { useState } from "react";

const RECORD_ROUTE = /^\/(approvals\/[^/]+|agents\/[^/]+\/(kill-switch|release|positions\/[^/]+\/close)|connections\/[^/]+\/(stop-all|close-all))\/?$/;

/**
 * The record screens: an approval request, a kill switch and its release, stopping or closing a
 * connection, and closing a position. What the owner reads there is fixed (`useFrozen`), so the
 * frame beside it shows the status strip, never the moving agent wire.
 */
export function isRecordRoute(pathname: string): boolean {
  return RECORD_ROUTE.test(pathname);
}

/**
 * A record screen's content, fixed at its first render (brief §4.1): what the owner confirms does
 * not change underneath them, and the stored artifact is built from the same value. Until they
 * confirm, a change in the live value marks the screen stale, and the screen asks for a fresh render
 * rather than updating silently. Once they confirm, the value never changes.
 */
export function useFrozen<T>(live: T | null, confirmed: boolean): { shown: T | null; stale: boolean; refresh: () => void } {
  const [frozen, setFrozen] = useState<{ value: T } | null>(null);
  if (frozen === null && live !== null) setFrozen({ value: live });
  const shown = frozen?.value ?? live;
  const stale = frozen !== null && !confirmed && JSON.stringify(live) !== JSON.stringify(frozen.value);
  return { shown, stale, refresh: () => setFrozen(live === null ? null : { value: live }) };
}
