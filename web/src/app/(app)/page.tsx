import type { Metadata } from "next";
import { HomeScreen } from "@/components/agent-chat/home";
import { WorkspaceGate } from "@/components/screens/common";

/** The root layout's title template does not apply to its own segment. */
export const metadata: Metadata = { title: { absolute: "Home · Owlhead" } };

export default function HomePage() {
  return (
    <WorkspaceGate api>
      <HomeScreen />
    </WorkspaceGate>
  );
}
