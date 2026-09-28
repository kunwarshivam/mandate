import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { AgentSectionScreen } from "@/components/screens/agent-detail";
import { AgentRecordScreen } from "@/components/screens/agent-records";
import { AGENT_ID, ASSET_ID, EVENT_ID, ORDER_ID } from "@/lib/ids";
import { type AgentRecord, RECORD_TITLE, findAgentRecord, findAgentSection } from "@/lib/screens";

interface Params {
  agentId: string;
  section: string[];
}

const RECORD_ID: Record<AgentRecord["kind"], RegExp> = {
  position: ASSET_ID,
  "close-position": ASSET_ID,
  order: ORDER_ID,
  decision: EVENT_ID,
};

function recordFor(segments: string[]): AgentRecord | undefined {
  const record = findAgentRecord(segments);
  return record && RECORD_ID[record.kind].test(record.id) ? record : undefined;
}

/** Generic on purpose: the title names the section or the kind of record, never the agent or the instrument (brief §5, rule 6). */
export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { section } = await params;
  const record = recordFor(section);
  if (record) return { title: RECORD_TITLE[record.kind] };
  return { title: findAgentSection(section)?.label ?? "Agent" };
}

export default async function AgentSectionPage({ params }: { params: Promise<Params> }) {
  const { agentId, section } = await params;
  if (!AGENT_ID.test(agentId)) notFound();
  const record = recordFor(section);
  if (record) return <AgentRecordScreen agentId={agentId} record={record} />;
  const found = findAgentSection(section);
  if (!found) notFound();
  return <AgentSectionScreen agentId={agentId} section={found.key} />;
}
