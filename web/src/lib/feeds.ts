import type { Workspace } from "@/fixtures/types";

/** Every feed answering: the only time the frame may leave out the status strip (DEC-215). */
export function allFeedsOk(ws: Workspace): boolean {
  return ws.status === "ready" && Object.values(ws.health).every((f) => f.state === "ok");
}
