import type { Metadata } from "next";
import { AgentsListScreen } from "@/components/screens/agents-list";

export const metadata: Metadata = { title: "Agents" };

export default function AgentsPage() {
  return <AgentsListScreen />;
}
