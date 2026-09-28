import type { Metadata } from "next";
import { Stub } from "@/components/screens/stub";

export const metadata: Metadata = { title: "Approvals" };

export default function ApprovalsPage() {
  return (
    <Stub title="Approvals">
      <p>Not built yet. It will list the requests waiting for your answer, each with its deadline. If you do nothing, the action is skipped.</p>
    </Stub>
  );
}
