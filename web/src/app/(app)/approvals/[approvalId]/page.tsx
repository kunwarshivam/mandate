import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { ApprovalRequestScreen } from "@/components/screens/approval-request";
import { APPROVAL_ID } from "@/lib/ids";

/** Generic on purpose: the instrument and agent stay out of the tab title and history. */
export const metadata: Metadata = { title: "Approval request" };

export default async function ApprovalPage({ params }: { params: Promise<{ approvalId: string }> }) {
  const { approvalId } = await params;
  if (!APPROVAL_ID.test(approvalId)) notFound();
  return <ApprovalRequestScreen approvalId={approvalId} />;
}
