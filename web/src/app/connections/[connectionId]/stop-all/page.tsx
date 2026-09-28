import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { StopRecordScreen } from "@/components/stop/record-screen";
import { getWorkspace } from "@/lib/get-workspace";
import { CONNECTION_ID } from "@/lib/ids";

/** A record screen names its environment in the title (brief §4.2), and never the agent or an instrument. */
export async function generateMetadata(): Promise<Metadata> {
  return { title: `Stop all agents (${(await getWorkspace()).environment})` };
}

export default async function StopAllPage({ params }: { params: Promise<{ connectionId: string }> }) {
  const { connectionId } = await params;
  if (!CONNECTION_ID.test(connectionId)) notFound();
  return <StopRecordScreen kind="stop_all" targetId={connectionId} />;
}
