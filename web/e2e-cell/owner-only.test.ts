// @vitest-environment node
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { writeOwnerOnly } from "./owner-only";

let dirs: string[] = [];
const scratch = () => {
  const dir = mkdtempSync(join(tmpdir(), "owner-only-"));
  dirs.push(dir);
  return dir;
};
const mode = (path: string) => statSync(path).mode & 0o777;

afterEach(() => {
  for (const dir of dirs) rmSync(dir, { recursive: true, force: true });
  dirs = [];
});

describe("writeOwnerOnly (DEC-2411's passkey hand-off)", () => {
  it("writes a new file owner-only", () => {
    const path = join(scratch(), "credential.json");
    writeOwnerOnly(path, "{}");
    expect(mode(path)).toBe(0o600);
    expect(readFileSync(path, "utf8")).toBe("{}");
  });

  it("replaces a stale world-readable file with an owner-only one holding the new content", () => {
    const path = join(scratch(), "credential.json");
    writeFileSync(path, "stale", { mode: 0o644 });
    expect(mode(path)).toBe(0o644);
    writeOwnerOnly(path, "fresh");
    expect(mode(path)).toBe(0o600);
    expect(readFileSync(path, "utf8")).toBe("fresh");
  });

  it("never writes through a planted symlink: its target is untouched and the path becomes a file of its own", () => {
    const dir = scratch();
    const target = join(dir, "target");
    writeFileSync(target, "theirs", { mode: 0o644 });
    const path = join(dir, "credential.json");
    symlinkSync(target, path);
    writeOwnerOnly(path, "ours");
    expect(readFileSync(target, "utf8")).toBe("theirs");
    expect(mode(target)).toBe(0o644);
    expect(lstatSync(path).isSymbolicLink()).toBe(false);
    expect(mode(path)).toBe(0o600);
    expect(readFileSync(path, "utf8")).toBe("ours");
  });

  it("refuses a directory at the path, and leaves it as it was", () => {
    const path = join(scratch(), "credential.json");
    mkdirSync(path);
    expect(() => writeOwnerOnly(path, "{}")).toThrow();
    expect(statSync(path).isDirectory()).toBe(true);
  });
});
