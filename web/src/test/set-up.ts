import { act } from "@testing-library/react";
import { vi } from "vitest";
import { confirmStepUpDigest } from "@/api/confirm";
import type { Creator } from "@/api/enrol";
import type { Authenticator } from "@/api/passkey";
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

/**
 * A stand-in authenticator for the API runtime (DEC-2128): it answers the mock member's passkey with
 * client data naming exactly the challenge it was asked to sign, as a browser's does. It signs
 * nothing, which the mock does not check. `calls` lists each challenge it was asked to sign.
 */
export function mockAuthenticator(passkeyId: string) {
  const calls: string[] = [];
  const get: Authenticator = async (options) => {
    const challenge = Buffer.from(new Uint8Array(options.publicKey?.challenge as ArrayBuffer)).toString("base64url");
    calls.push(challenge);
    const client = new TextEncoder().encode(JSON.stringify({ type: "webauthn.get", challenge, origin: "https://app.owlhead.ai" }));
    return { type: "public-key", id: passkeyId, rawId: Uint8Array.from(Buffer.from(passkeyId, "base64url")).buffer, response: { authenticatorData: Uint8Array.of(1).buffer, clientDataJSON: client.buffer, signature: Uint8Array.of(2).buffer } } as unknown as Credential;
  };
  return { get, calls };
}

/**
 * A stand-in authenticator that makes a credential (DEC-2108 item 4): client data naming exactly the
 * challenge it was asked to sign, as a `webauthn.create`, and a fixed 16-byte id. It attests nothing,
 * which the mock does not check. `calls` lists each creation's options.
 */
export function mockCreator(rawId: Uint8Array = Uint8Array.from({ length: 16 }, (_, i) => 50 + i)) {
  const calls: CredentialCreationOptions[] = [];
  const create: Creator = async (options) => {
    calls.push(options);
    const challenge = Buffer.from(new Uint8Array(options.publicKey?.challenge as ArrayBuffer)).toString("base64url");
    const client = new TextEncoder().encode(JSON.stringify({ type: "webauthn.create", challenge, origin: "https://app.owlhead.ai" }));
    return { type: "public-key", id: Buffer.from(rawId).toString("base64url"), rawId: rawId.slice().buffer, response: { attestationObject: Uint8Array.of(0xa3, 1, 2).buffer, clientDataJSON: client.buffer } } as unknown as Credential;
  };
  return { create, calls, passkeyId: Buffer.from(rawId).toString("base64url") };
}
