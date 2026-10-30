import type { Metadata } from "next";
import { Suspense } from "react";
import { SetupScreen } from "@/components/agent-chat/setup-screen";
import { WorkspaceGate } from "@/components/screens/common";

export const metadata: Metadata = { title: "New agent" };

export default function NewAgentPage() {
  return (
    <Suspense>
      <WorkspaceGate api>
        <SetupScreen />
      </WorkspaceGate>
    </Suspense>
  );
}
