import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { AgentSectionScreen } from "@/components/screens/agent-detail";
import { AGENT_ID } from "@/lib/ids";
import { findAgentSection } from "@/lib/screens";

interface Params {
  agentId: string;
  section: string[];
}

/** Generic on purpose: the title names the section, never the agent (brief §5, rule 6). */
export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { section } = await params;
  return { title: findAgentSection(section)?.label ?? "Agent" };
}

export default async function AgentSectionPage({ params }: { params: Promise<Params> }) {
  const { agentId, section } = await params;
  const found = findAgentSection(section);
  if (!AGENT_ID.test(agentId) || !found) notFound();
  return <AgentSectionScreen agentId={agentId} section={found.key} />;
}
