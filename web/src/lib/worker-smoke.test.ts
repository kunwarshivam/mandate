// @vitest-environment node
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import nextConfig from "../../next.config";
import { SMOKE_REQUESTS, hostingProblems, smokeCommand, smokeProblems } from "../../scripts/workers.mjs";

/**
 * The pre-deploy smoke (2026-10-29's outage): `web.yml` boots the Worker it built under `wrangler dev`
 * and asks the public site and the app for `/`; anything but a 200 fails the pull request, naming the
 * error the Worker answered, before `web-deploy.yml` can ship it.
 */
const WEB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const real = (path: string) => readFileSync(join(WEB, path), "utf8");
const only = (fragment: string) => [expect.stringContaining(fragment)];
const ok = (host: string) => ({ host, path: "/", status: 200, body: "<html></html>" });
const THROWN = "Error: Unexpected loadManifest(/.next/server/preview-props.json) call!\n    at loadManifest (handler.mjs:343:35610)";

function tree(changes: Record<string, string> = {}) {
  return {
    exists: (path: string) => path in changes || existsSync(join(WEB, path)),
    read: (path: string) => changes[path] ?? readFileSync(join(WEB, path), "utf8"),
    nextConfig: { images: nextConfig.images ?? undefined },
  };
}

describe("the built Worker answers the public site and the app before it is deployed", () => {
  it("asks both hosts for their root", () => {
    expect(SMOKE_REQUESTS).toEqual([
      { host: "owlhead.ai", path: "/" },
      { host: "app.owlhead.ai", path: "/" },
    ]);
  });

  it("passes two 200s and fails any other answer, naming the host and the error the Worker threw", () => {
    expect(smokeProblems([ok("owlhead.ai"), ok("app.owlhead.ai")])).toEqual([]);
    const thrown = smokeProblems([{ host: "owlhead.ai", path: "/", status: 500, body: THROWN }, ok("app.owlhead.ai")]);
    expect(thrown).toEqual(only("owlhead.ai/ answered 500"));
    expect(thrown[0]).toContain("Unexpected loadManifest(/.next/server/preview-props.json) call!");
    expect(thrown[0]).not.toContain("handler.mjs");
    expect(smokeProblems([ok("owlhead.ai"), { host: "app.owlhead.ai", path: "/", status: 404, body: "" }])).toEqual(only("app.owlhead.ai/ answered 404"));
    expect(smokeProblems([ok("owlhead.ai")])).toEqual(only("app.owlhead.ai/ was not asked"));
  });

  it("smoke exits 0 when the Worker answers both, 1 when it does not or never starts, 2 for a bad command line", async () => {
    const serving = (status: number) => async () => SMOKE_REQUESTS.map((r) => ({ ...r, status, body: status === 200 ? "" : THROWN }));
    expect((await smokeCommand(["smoke"], serving(200))).status).toBe(0);
    const failed = await smokeCommand(["smoke"], serving(500));
    expect(failed.status).toBe(1);
    expect(failed.lines.join("\n")).toContain("Unexpected loadManifest");
    const never = await smokeCommand(["smoke"], async () => {
      throw new Error("wrangler dev was not ready within 90 s");
    });
    expect(never).toEqual({ status: 1, lines: [expect.stringContaining("not ready")] });
    expect((await smokeCommand(["smoke", "extra"], serving(200))).status).toBe(2);
    expect((await smokeCommand([], serving(200))).status).toBe(2);
  });

  it("is a step of web.yml, the pull-request build, after the Worker is built", () => {
    const workflow = "../.github/workflows/web.yml";
    const yml = real(workflow);
    expect(hostingProblems(tree())).toEqual([]);
    expect(yml.indexOf("node scripts/workers.mjs smoke")).toBeGreaterThan(yml.indexOf("npm run cf:build"));
    expect(hostingProblems(tree({ [workflow]: yml.replace(/scripts\/workers\.mjs smoke/g, "true") }))).toEqual(only("workers.mjs smoke"));
  });
});
