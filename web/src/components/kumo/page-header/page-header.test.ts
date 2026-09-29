import { describe, expect, it } from "vitest";
import { isCurrentTab } from "./page-header";

const TABS = ["", "/positions", "/orders", "/prove", "/prove/replay"].map((s) => ({ href: `/agents/agt_1${s}`, label: s || "Overview" }));
const current = (pathname: string) => TABS.filter((t) => isCurrentTab(pathname, t.href, TABS)).map((t) => t.label);

describe("isCurrentTab", () => {
  it("marks only the exact tab on a section page, never the overview as well", () => {
    expect(current("/agents/agt_1")).toEqual(["Overview"]);
    expect(current("/agents/agt_1/orders")).toEqual(["/orders"]);
    expect(current("/agents/agt_1/prove/replay")).toEqual(["/prove/replay"]);
  });

  it("marks the deepest tab a nested page sits under", () => {
    expect(current("/agents/agt_1/orders/ord_9")).toEqual(["/orders"]);
    expect(current("/agents/agt_1/prove/replay/run_2")).toEqual(["/prove/replay"]);
    expect(current("/agents/agt_1/decisions/dec_3")).toEqual(["Overview"]);
  });
});
