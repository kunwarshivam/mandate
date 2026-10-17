import type { Metadata } from "next";
import { AgentsListScreen } from "@/components/screens/agents-list";
import { ApiAgents, BySource } from "@/components/api-screens/api-screens";

export const metadata: Metadata = { title: "Agents" };

export default function AgentsPage() {
  return <BySource fixtures={<AgentsListScreen />} api={<ApiAgents />} />;
}
