import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { ApprovalRequestScreen } from "@/components/screens/approval-request";
import { APPROVAL_ID, EVENT_ID } from "@/lib/ids";
import { ApiApproval, BySource } from "@/components/api-screens/api-screens";

/** Generic on purpose: the instrument and agent stay out of the tab title and history. */
export const metadata: Metadata = { title: "Approval request" };

/** The fixtures' ids carry a type prefix; the workspace API's are the bare ULIDs of `ApprovalRequested` (DEC-1096). */
export default async function ApprovalPage({ params }: { params: Promise<{ approvalId: string }> }) {
  const { approvalId } = await params;
  if (!APPROVAL_ID.test(approvalId) && !EVENT_ID.test(approvalId)) notFound();
  return <BySource fixtures={<ApprovalRequestScreen approvalId={approvalId} />} api={<ApiApproval key={approvalId} approvalId={approvalId} />} />;
}
