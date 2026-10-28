import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { API_ORIGIN_ENV, DATA_SOURCE_ENV, WORKSPACE_ENV, readDataSource } from "./data-source";

const SRC = join(__dirname, "..");
const WS = "ws_01J9ZQ4M0000000000000000AB";
const api = (origin: string, workspace: string | undefined) => readDataSource({ [DATA_SOURCE_ENV]: "api", [API_ORIGIN_ENV]: origin, [WORKSPACE_ENV]: workspace });

function sources(): Array<{ path: string; text: string }> {
  return readdirSync(SRC, { recursive: true })
    .map((name) => join(SRC, String(name)))
    .filter((path) => /\.(ts|tsx)$/.test(path) && !/\.test\.tsx?$/.test(path))
    .map((path) => ({ path: relative(SRC, path), text: readFileSync(path, "utf8") }));
}

describe("the data source (DEC-975)", () => {
  it("is the fixtures unless the server's configuration names the API", () => {
    expect(readDataSource({})).toEqual({ kind: "fixtures" });
    expect(readDataSource({ [DATA_SOURCE_ENV]: "" })).toEqual({ kind: "fixtures" });
    expect(readDataSource({ [DATA_SOURCE_ENV]: "fixtures", [API_ORIGIN_ENV]: "https://api.owlhead.ai", [WORKSPACE_ENV]: WS })).toEqual({ kind: "fixtures" });
  });

  it("is the API at an https origin, or a loopback http one for development and the e2e suite", () => {
    expect(api("https://api.owlhead.ai", WS)).toEqual({ kind: "api", origin: "https://api.owlhead.ai", workspaceId: WS });
    expect(api("https://api.owlhead.ai/", WS)).toEqual({ kind: "api", origin: "https://api.owlhead.ai", workspaceId: WS });
    expect(api("http://127.0.0.1:4319", WS)).toEqual({ kind: "api", origin: "http://127.0.0.1:4319", workspaceId: WS });
    expect(api("http://localhost:4319", WS)).toEqual({ kind: "api", origin: "http://localhost:4319", workspaceId: WS });
  });

  it("leaves the workspace to the session's memberships when none is named, and only then", () => {
    expect(api("https://api.owlhead.ai", undefined)).toEqual({ kind: "api", origin: "https://api.owlhead.ai", workspaceId: null });
    expect(api("https://api.owlhead.ai", "").kind).toBe("misconfigured");
  });

  it("is misconfigured, never the fixtures, when the API is chosen and its origin or workspace does not parse", () => {
    for (const origin of ["", "api.owlhead.ai", "http://api.owlhead.ai", "https://api.owlhead.ai/v1", "https://api.owlhead.ai?x=1", "https://user:pw@api.owlhead.ai", "ftp://api.owlhead.ai", "javascript:alert(1)"])
      expect(api(origin, WS).kind, origin).toBe("misconfigured");
    for (const workspace of ["", "../ws", "ws/1", "ws 1"]) expect(api("https://api.owlhead.ai", workspace).kind, String(workspace)).toBe("misconfigured");
    for (const value of ["API", "Api", "live", "fixture"]) expect(readDataSource({ [DATA_SOURCE_ENV]: value }).kind, value).toBe("misconfigured");
  });

  it("is never chosen by a variable the build inlines into the bundle (ES-12)", () => {
    expect(readDataSource({ [`NEXT_PUBLIC_${DATA_SOURCE_ENV}`]: "api", [`NEXT_PUBLIC_${API_ORIGIN_ENV}`]: "https://api.owlhead.ai", [`NEXT_PUBLIC_${WORKSPACE_ENV}`]: WS })).toEqual({ kind: "fixtures" });
    for (const { path, text } of sources()) expect(text, path).not.toMatch(/NEXT_PUBLIC_OWLHEAD_(DATA_SOURCE|API|WORKSPACE)/);
    expect(readFileSync(join(SRC, "..", "next.config.ts"), "utf8")).not.toMatch(/OWLHEAD_(DATA_SOURCE|API_ORIGIN|WORKSPACE_ID)/);
  });

  it("is read only on the server: no client module imports it except for its types", () => {
    for (const { path, text } of sources()) {
      if (!/^["']use client["']/.test(text.trimStart())) continue;
      for (const line of text.split("\n").filter((l) => /from ["'](@\/lib\/data-source|\.\/data-source|\.\.\/lib\/data-source)["']/.test(l)))
        expect(line, path).toMatch(/^import type /);
    }
  });
});
