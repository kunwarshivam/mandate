import { describe, expect, it } from "vitest";
import { createWorkspaceClient } from "./client";
import { prepareOwnerExit, recordRef } from "./commands";
import type { CommandRecord } from "@/lib/mock-runtime";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ASSET = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const RECORD: CommandRecord = { screen: "D10", environment: "paper", title: "Exit", shown: ["Sell all 10 of XYZ."], modes: [] };
const client = createWorkspaceClient({ workspaceId: WS, fetch: () => Promise.reject(new Error("no network")), baseUrl: "https://api.owlhead.ai" });

describe("an owner exit (spec §5.4, rule 13)", () => {
  it("names the asset and the screen shown, with no bid confirmation and no step-up, on the agent's exits route", () => {
    const prepared = prepareOwnerExit(client, "agt_1", ASSET, RECORD);
    if (!prepared.ok) throw new Error(prepared.reason);
    expect(prepared.commands).toHaveLength(1);
    const [command] = prepared.commands;
    expect(command.operation).toBe("owner_exit");
    expect(command.path).toBe(`/v1/workspaces/${WS}/agents/agt_1/exits`);
    expect(JSON.parse(command.body)).toEqual({ instrument: ASSET, bid: null, bid_size: null, quoted_at: null, floor: null, record: recordRef(RECORD), step_up: null });
  });

  it("sends nothing for an instrument that is not an asset id, since the route takes nothing else", () => {
    for (const instrument of ["XYZ", ASSET.toUpperCase(), `${ASSET} `, ""]) expect(prepareOwnerExit(client, "agt_1", instrument, RECORD)).toEqual({ ok: false, reason: "no_asset_id" });
  });

  it("sends nothing without an agent", () => {
    expect(prepareOwnerExit(client, "", ASSET, RECORD)).toEqual({ ok: false, reason: "no_agent" });
  });

  it("keys each gesture once: a second exit is a new command, the same one is not", () => {
    const one = prepareOwnerExit(client, "agt_1", ASSET, RECORD);
    const two = prepareOwnerExit(client, "agt_1", ASSET, RECORD);
    if (!one.ok || !two.ok) throw new Error("prepared");
    expect(one.commands[0].idempotency_key).not.toBe(two.commands[0].idempotency_key);
  });
});
