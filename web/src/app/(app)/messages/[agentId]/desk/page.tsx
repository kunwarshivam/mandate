import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { MessagesScreen } from "@/components/messages/messages-screen";
import { AGENT_ID } from "@/lib/ids";

/** Generic on purpose: an agent's name often contains a ticker (brief §5, rule 6). */
export const metadata: Metadata = { title: "Messages" };

export default async function DeskPage({ params }: { params: Promise<{ agentId: string }> }) {
  const { agentId } = await params;
  if (!AGENT_ID.test(agentId)) notFound();
  return <MessagesScreen agentId={agentId} view="desk" />;
}
