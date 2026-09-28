import { describe, expect, it } from "vitest";
import { cn } from "./utils";

describe("cn", () => {
  it("lets a type-scale size replace a primitive's default size", () => {
    expect(cn("text-base font-medium", "text-title")).toBe("font-medium text-title");
    expect(cn("text-sm", "text-caption")).toBe("text-caption");
  });

  it("keeps a colour and a type-scale size together", () => {
    expect(cn("text-foreground", "text-heading")).toBe("text-foreground text-heading");
  });
});
