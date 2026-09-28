import { useState } from "react";

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
