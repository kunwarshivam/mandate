import type { Metadata } from "next";
import { Stub } from "@/components/screens/stub";

export const metadata: Metadata = { title: "Audit" };

export default function AuditPage() {
  return (
    <Stub title="Audit">
      <p>Not built in this slice. It will hold each agent&apos;s timeline, the causal trace from any order back to its causes, exports, and journal chain verification.</p>
      <p>Until then, each agent&apos;s recent timeline and gate decisions are on its page.</p>
    </Stub>
  );
}
