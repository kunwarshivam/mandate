import { act } from "@testing-library/react";
import { vi } from "vitest";
import { confirmStepUpDigest } from "@/api/confirm";
import type { NewAgent } from "@/lib/fixture-journey";
import type { ConfirmRecord, DeployToPaperRecord } from "@/lib/mock-runtime";
import { RECORD_AFTER_MS } from "./harness";
import { probed } from "./runtime-probe";

/** A new agent's A5 record, as a test sends it to the runtime directly. */
export const CONFIRM_RECORD: ConfirmRecord = {
  screen: "A5",
  environment: "paper",
  shown: ["Confirm your mandate"],
  paths: ["/capital/allocation_usd"],
  classification: "risk_increasing",
};
export const DEPLOY_RECORD: DeployToPaperRecord = {
  screen: "B3",
  environment: "paper",
  shown: ["Deploy to paper"],
  backtestId: null,
};

/**
 * Sets up an agent through the runtime the way the screen does (DEC-2401): creates its version, then,
 * once that is recorded, confirms it with a passkey bound to it, then, once that is recorded, sends
 * the deployment. Leaves the deployment sent; the caller advances the clock to record it. Fake timers.
 */
export function setUpThrough(request: NewAgent): string {
  let id = "";
  act(() => {
    id = probed().createVersion(request).id;
  });
  act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
  const version = probed().setUps.find((s) => s.id === id)!.version;
  act(() => probed().confirmVersion(id, CONFIRM_RECORD, confirmStepUpDigest(version, null, null)));
  act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
  act(() => probed().deployVersion(id, DEPLOY_RECORD));
  return id;
}
