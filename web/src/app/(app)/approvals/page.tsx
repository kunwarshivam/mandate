import type { Metadata } from "next";
import { ApprovalsInboxScreen } from "@/components/screens/approvals-inbox";
import { ApiApprovals, BySource } from "@/components/api-screens/api-screens";

export const metadata: Metadata = { title: "Approvals" };

export default function ApprovalsPage() {
  return <BySource fixtures={<ApprovalsInboxScreen />} api={<ApiApprovals />} />;
}
