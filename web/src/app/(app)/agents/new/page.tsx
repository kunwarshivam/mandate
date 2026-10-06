import type { Metadata } from "next";
import { NewAgentFlow } from "@/components/new-agent/new-agent-flow";
import { WorkspaceGate } from "@/components/screens/common";

export const metadata: Metadata = { title: "New agent" };

export default function NewAgentPage() {
  return (
    <WorkspaceGate>
      <NewAgentFlow />
    </WorkspaceGate>
  );
}
