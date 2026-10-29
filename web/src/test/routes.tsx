import type { ComponentType } from "react";
import Design from "@/app/(app)/design/page.dev";
import ErrorScreen from "@/app/(app)/error";
import Loading from "@/app/(app)/loading";
import NotFound from "@/app/(app)/not-found";
import Setup from "@/app/(app)/new/page";
import Home from "@/app/(app)/page";
import Settings from "@/app/(app)/settings/page";
import { recordHref } from "@/components/stop/commands";
import { StopRecordScreen } from "@/components/stop/record-screen";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";

const CONNECTION = buildWorkspace("normal").connection.connection_id;

/** Every screen the app renders, by the pathname the shell sees for it. */
export const ROUTES: Array<[string, ComponentType]> = [
  ["/", Home],
  ["/new", Setup],
  ["/settings", Settings],
  ["/design", Design],
  ["/loading", Loading],
  ["/not-found", NotFound],
  ["/error", () => <ErrorScreen error={new Error("render failed")} reset={() => {}} />],
  [recordHref("kill", AGENT_IDS.btc), () => <StopRecordScreen kind="kill" targetId={AGENT_IDS.btc} />],
  [recordHref("release", AGENT_IDS.btc), () => <StopRecordScreen kind="release" targetId={AGENT_IDS.btc} />],
  [recordHref("stop_all", CONNECTION), () => <StopRecordScreen kind="stop_all" targetId={CONNECTION} />],
  [recordHref("close_all", CONNECTION), () => <StopRecordScreen kind="close_all" targetId={CONNECTION} />],
];
