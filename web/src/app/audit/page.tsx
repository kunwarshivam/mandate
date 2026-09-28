import type { Metadata } from "next";
import { SectionIndexScreen } from "@/components/screens/account-screens";
import { SECTION_INDEX } from "@/lib/screens";

export const metadata: Metadata = { title: "Audit" };

export default function AuditPage() {
  return <SectionIndexScreen title={SECTION_INDEX.audit.label} purpose={SECTION_INDEX.audit.purpose} prefix="/audit" />;
}
