import { connection } from "next/server";

/**
 * Where the app's screens read from and send to (DEC-970): the recorded fixtures, or one workspace
 * deployment's API (workspace API spec §3). Chosen by server-side configuration alone, read when a
 * request is served and never inlined into the bundle (ES-12): `OWLHEAD_DATA_SOURCE` (`fixtures`, the
 * default, or `api`), `OWLHEAD_API_ORIGIN` and `OWLHEAD_WORKSPACE_ID`. Neither is a secret; the browser
 * reaches the API with its own session cookie (spec §3.3).
 *
 * An `api` choice whose origin or workspace does not parse, or any other value, is `misconfigured`:
 * the app shows the deployment unreachable rather than falling back to fixtures, so fixture figures
 * never pass for a record (rule 3).
 */
export type DataSource =
  | { kind: "fixtures" }
  | { kind: "api"; origin: string; workspaceId: string }
  | { kind: "misconfigured"; reason: string };

/** What the browser is told: the API's origin and the workspace, never anything else. */
export type ApiSource = Extract<DataSource, { kind: "api" }>;

export const DATA_SOURCE_ENV = "OWLHEAD_DATA_SOURCE";
export const API_ORIGIN_ENV = "OWLHEAD_API_ORIGIN";
export const WORKSPACE_ENV = "OWLHEAD_WORKSPACE_ID";

export function readDataSource(env: Readonly<Record<string, string | undefined>>): DataSource {
  void env;
  throw new Error("Unimplemented: E11-9");
}

/** The source for this request. `connection()` keeps the read at request time, so a build never fixes it. */
export async function getDataSource(): Promise<DataSource> {
  await connection();
  return readDataSource(process.env);
}
