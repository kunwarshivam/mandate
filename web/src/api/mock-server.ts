/**
 * A fixture-backed stand-in for the workspace API: a `fetch` implementation for tests and for
 * `npm run dev`. It answers from `src/fixtures/` in the shapes of the workspace API spec, keeps
 * commands in memory by idempotency key (API-4), and plays the `unreachable` and `result-unknown`
 * scenarios as a network that does not answer.
 */
import { UNIMPLEMENTED } from "./unimplemented";
import type { Fetch } from "./client";
import type { Scenario } from "@/fixtures/types";

export interface MockServerOptions {
  workspaceId: string;
  scenario?: Scenario;
}

export interface RecordedCommand {
  event_id: string;
  path: string;
  idempotency_key: string;
  body: string;
}

export interface MockServer {
  fetch: Fetch;
  /** Commands the mock has recorded, in order. */
  recorded(): RecordedCommand[];
}

export function createMockServer(options: MockServerOptions): MockServer {
  void options;
  throw new Error(UNIMPLEMENTED);
}
