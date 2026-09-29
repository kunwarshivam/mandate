import type { Metadata } from "next";
import { SectionIndexScreen } from "@/components/screens/account-screens";
import { SECTION_INDEX } from "@/lib/screens";

export const metadata: Metadata = { title: "Workspace" };

export default function SettingsPage() {
  return <SectionIndexScreen title={SECTION_INDEX.workspace.label} purpose={SECTION_INDEX.workspace.purpose} prefix="/settings" />;
}
