import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { recordHref } from "@/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "@/fixtures/workspace";
import { agentHref } from "./screens";
import { isRecordRoute, useFrozen } from "./frozen";

describe("isRecordRoute", () => {
  it("knows the record screens: an approval, the kill switch and release, stopping or closing a connection", () => {
    for (const path of [
      `/approvals/${APPROVAL_IDS.swingXyz}`,
      recordHref("kill", AGENT_IDS.btc),
      recordHref("release", AGENT_IDS.btc),
      recordHref("stop_all", "con_1"),
      recordHref("close_all", "con_1"),
    ])
      expect(isRecordRoute(path), path).toBe(true);
  });

  it("leaves every live screen live", () => {
    for (const path of ["/", "/new", "/settings", agentHref(AGENT_IDS.btc), `/agents/${AGENT_IDS.swing}/positions/asset_1/close`, "/connections", "/approvals/apr_1/more"])
      expect(isRecordRoute(path), path).toBe(false);
  });
});

describe("useFrozen (brief §4.1)", () => {
  const setup = (live: { mode: string } | null, confirmed = false) =>
    renderHook(({ live, confirmed }) => useFrozen(live, confirmed), { initialProps: { live, confirmed } });

  it("shows the value from the first render, and marks it stale when it changes before confirming", () => {
    const hook = setup({ mode: "normal" });
    hook.rerender({ live: { mode: "paused" }, confirmed: false });
    expect(hook.result.current.shown).toEqual({ mode: "normal" });
    expect(hook.result.current.stale).toBe(true);

    act(() => hook.result.current.refresh());
    expect(hook.result.current.shown).toEqual({ mode: "paused" });
    expect(hook.result.current.stale).toBe(false);
  });

  it("never changes and is never stale once confirmed", () => {
    const hook = setup({ mode: "normal" });
    hook.rerender({ live: { mode: "normal" }, confirmed: true });
    hook.rerender({ live: { mode: "stopped" }, confirmed: true });
    expect(hook.result.current.shown).toEqual({ mode: "normal" });
    expect(hook.result.current.stale).toBe(false);
  });

  it("marks it stale when the subject disappears, and a fresh render shows nothing", () => {
    const hook = setup({ mode: "normal" });
    hook.rerender({ live: null, confirmed: false });
    expect(hook.result.current.stale).toBe(true);
    act(() => hook.result.current.refresh());
    expect(hook.result.current.shown).toBeNull();
  });

  it("freezes once the value first exists, as after loading", () => {
    const hook = setup(null);
    expect(hook.result.current.shown).toBeNull();
    hook.rerender({ live: { mode: "normal" }, confirmed: false });
    hook.rerender({ live: { mode: "paused" }, confirmed: false });
    expect(hook.result.current.shown).toEqual({ mode: "normal" });
    expect(hook.result.current.stale).toBe(true);
  });
});
