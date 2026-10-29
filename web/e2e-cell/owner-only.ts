import { rmSync, writeFileSync } from "node:fs";

/**
 * Writes `text` to `path` so that only its owner can ever read it (DEC-2411's passkey hand-off):
 * whatever is at `path` is removed first, a stale file or a planted symlink alike, never followed,
 * and the new file is created exclusively at 0600 (`wx`), so its content never sits in a file
 * another user can read, not even for a moment. A directory at `path` refuses.
 */
export function writeOwnerOnly(path: string, text: string): void {
  rmSync(path, { force: true });
  writeFileSync(path, text, { mode: 0o600, flag: "wx" });
}
