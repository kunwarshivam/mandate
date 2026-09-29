import type { Workspace } from "@/fixtures/types";

export type FeedKey = keyof Workspace["health"];

/** The feed heard from longest ago, whose age is how fresh the whole screen is. */
export function oldestFeed(ws: Workspace): { key: FeedKey; as_of: string } {
  const feeds = Object.entries(ws.health) as Array<[FeedKey, { as_of: string }]>;
  const [key, feed] = feeds.reduce((a, b) => (Date.parse(b[1].as_of) < Date.parse(a[1].as_of) ? b : a));
  return { key, as_of: feed.as_of };
}

/** Every feed answering: the only time the frame may show the agent wire instead of the status strip. */
export function allFeedsOk(ws: Workspace): boolean {
  return ws.status === "ready" && Object.values(ws.health).every((f) => f.state === "ok");
}
