import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface VercelPolicy {
  git: {
    deploymentEnabled: Record<string, boolean>;
  };
}

const policy = JSON.parse(readFileSync(resolve(process.cwd(), "../vercel.json"), "utf8")) as VercelPolicy;

describe("Vercel deployment policy", () => {
  it("disables every preview branch, including slash-named agent branches", () => {
    expect(policy.git.deploymentEnabled).toEqual({
      "**": false,
      main: true,
    });
  });
});
