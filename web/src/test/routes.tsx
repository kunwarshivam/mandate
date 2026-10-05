import type { ComponentType } from "react";
import AgentsNew from "@/app/(app)/agents/new/page";
import Agents from "@/app/(app)/agents/page";
import Approvals from "@/app/(app)/approvals/page";
import Audit from "@/app/(app)/audit/page";
import DesignNewAgent from "@/app/(app)/design/new-agent/page";
import Design from "@/app/(app)/design/page";
import ErrorScreen from "@/app/(app)/error";
import Loading from "@/app/(app)/loading";
import NotFound from "@/app/(app)/not-found";
import Dashboard from "@/app/(app)/page";
import Settings from "@/app/(app)/settings/page";
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
  ["/design/new-agent", DesignNewAgent],
  ["/loading", Loading],
  ["/not-found", NotFound],
  ["/error", () => <ErrorScreen error={new Error("render failed")} reset={() => {}} />],
  [recordHref("kill", AGENT_IDS.btc), () => <StopRecordScreen kind="kill" targetId={AGENT_IDS.btc} />],
  [recordHref("release", AGENT_IDS.btc), () => <StopRecordScreen kind="release" targetId={AGENT_IDS.btc} />],
  [recordHref("stop_all", CONNECTION), () => <StopRecordScreen kind="stop_all" targetId={CONNECTION} />],
  [recordHref("close_all", CONNECTION), () => <StopRecordScreen kind="close_all" targetId={CONNECTION} />],
];
