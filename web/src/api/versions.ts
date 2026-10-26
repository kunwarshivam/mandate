/**
 * Creating a mandate version and reading its confirmation screen over the workspace API (spec §3.11
 * item 11, §4.1; DEC-2101 items 3 and 4). The version is created from the mandate document itself,
 * as the CLI does, and the confirmation is read back from the server, so the confirm binds the
 * server's own `screen_digest` and classification, never values computed here.
 */
import type { WorkspaceClient } from "./client";
import { CLASSIFICATIONS } from "./confirm";
import type { ServedConfirmation } from "./commands";
import { array, closedEnum, contentRef, isRecord, object, string } from "./decode";
import type { ContentRef, Decoded, Decoder, JsonObject, PreparedCommand, ReadOutcome } from "./types";
import type { Mandate } from "@/fixtures/types";

/** `POST /mandate-versions`: the body is the mandate document, every envelope path entered by the owner (DEC-2101 item 3). */
export function prepareCreateVersion(client: WorkspaceClient, mandate: Mandate): PreparedCommand {
  return client.prepare("create_version", {}, structuredClone(mandate) as unknown as JsonObject);
}

/** `commands/version-created`, as far as the set-up needs it: the version the server hashed, and the warnings' codes. */
export interface VersionCreated {
  mandate_version: ContentRef;
  warnings: string[];
}

const created = object<VersionCreated>({ mandate_version: contentRef, warnings: array(string) });

/** The 202 (or the 200 of a version already recorded) decoded; anything else is not a version. */
export function versionCreated(body: unknown): Decoded<VersionCreated> {
  return isRecord(body) ? created(body, "") : { ok: false, issue: { path: "", problem: "wrong_type", value: null, allowed: null } };
}

/** What `GET .../confirmation` serves that the confirm binds and A5 shows (DEC-2101 item 4). */
export interface Confirmation extends ServedConfirmation {
  mandate_version: ContentRef;
  required_paths: string[];
  warnings: string[];
  /** Whether the confirm needs step-up, as the server judged it for this version (a risk-increasing one does). */
  step_up_required: boolean;
}

const boolean: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : { ok: false, issue: { path, problem: "wrong_type", value: null, allowed: null } });

const confirmation = object<Confirmation>({
  mandate_version: contentRef,
  classification: closedEnum(CLASSIFICATIONS),
  required_paths: array(string),
  warnings: array(string),
  step_up_required: boolean,
  screen_digest: contentRef,
});

/** The served confirmation screen for `version`; a version the server does not hold is its `not_found`. */
export function readConfirmation(client: WorkspaceClient, version: ContentRef): Promise<ReadOutcome<Confirmation>> {
  return client.read(`/mandate-versions/${encodeURIComponent(version)}/confirmation`, (body, path) => {
    const read = confirmation(body, path);
    if (read.ok && read.value.mandate_version !== version) return { ok: false, issue: { path: `${path}/mandate_version`, problem: "wrong_type", value: read.value.mandate_version, allowed: [version] } };
    return read;
  });
}
