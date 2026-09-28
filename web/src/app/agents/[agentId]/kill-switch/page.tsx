import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { StopRecordScreen } from "@/components/stop/record-screen";
import { getWorkspace } from "@/lib/get-workspace";
import { AGENT_ID } from "@/lib/ids";

/** A record screen names its environment in the title (brief §4.2), and never the agent or an instrument. */
export async function generateMetadata(): Promise<Metadata> {
  return { title: `Kill switch (${(await getWorkspace()).environment})` };
}

export default async function KillSwitchPage({ params }: { params: Promise<{ agentId: string }> }) {
  const { agentId } = await params;
  if (!AGENT_ID.test(agentId)) notFound();
  return <StopRecordScreen kind="kill" targetId={agentId} />;
}
