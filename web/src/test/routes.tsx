import type { ComponentType } from "react";
import AgentsNew from "@/app/agents/new/page";
import Agents from "@/app/agents/page";
import Approvals from "@/app/approvals/page";
import Audit from "@/app/audit/page";
import Design from "@/app/design/page";
import ErrorScreen from "@/app/error";
import Loading from "@/app/loading";
import NotFound from "@/app/not-found";
import Dashboard from "@/app/page";
import Settings from "@/app/settings/page";
import { recordHref } from "@/components/stop/commands";
import { StopRecordScreen } from "@/components/stop/record-screen";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";

const CONNECTION = buildWorkspace("normal").connection.connection_id;

/** Every screen the app renders, by the pathname the shell sees for it. */
export const ROUTES: Array<[string, ComponentType]> = [
  ["/", Dashboard],
  ["/agents", Agents],
  ["/agents/new", AgentsNew],
  ["/approvals", Approvals],
  ["/audit", Audit],
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
