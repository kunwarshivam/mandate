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
];
