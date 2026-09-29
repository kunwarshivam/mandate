import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { AgentDetailScreen } from "@/components/screens/agent-detail";
import { AGENT_ID } from "@/lib/ids";

/** Generic on purpose: an agent's name often contains a ticker (brief §5, rule 6). */
export const metadata: Metadata = { title: "Agent" };

export default async function AgentPage({ params }: { params: Promise<{ agentId: string }> }) {
  const { agentId } = await params;
  if (!AGENT_ID.test(agentId)) notFound();
  return <AgentDetailScreen agentId={agentId} />;
}
