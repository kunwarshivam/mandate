import { describe, expect, it } from "vitest";
import { cn } from "./utils";

describe("cn", () => {
  it("lets a type-scale size replace a primitive's default size", () => {
    expect(cn("text-base font-medium", "text-h1")).toBe("font-medium text-h1");
    expect(cn("text-sm", "text-caption")).toBe("text-caption");
    expect(cn("text-lg", "text-hero")).toBe("text-hero");
    expect(cn("text-hero", "text-display")).toBe("text-display");
  });

  it("keeps a colour and a type-scale size together", () => {
    expect(cn("text-foreground", "text-h2")).toBe("text-foreground text-h2");
    expect(cn("text-muted-foreground", "text-display")).toBe("text-muted-foreground text-display");
  });
});
