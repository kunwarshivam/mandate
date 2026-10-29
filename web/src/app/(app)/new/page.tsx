import type { Metadata } from "next";
import { SetupScreen } from "@/components/agent-chat/setup-screen";

export const metadata: Metadata = { title: "New agent" };

export default function NewAgentPage() {
  return <SetupScreen />;
}
