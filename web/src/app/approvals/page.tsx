import type { Metadata } from "next";
import { ApprovalsInboxScreen } from "@/components/screens/approvals-inbox";

export const metadata: Metadata = { title: "Approvals" };

export default function ApprovalsPage() {
  return <ApprovalsInboxScreen />;
}
