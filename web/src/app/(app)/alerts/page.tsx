import type { Metadata } from "next";
import { AlertsScreen } from "@/components/screens/account-screens";

export const metadata: Metadata = { title: "Alerts" };

export default function AlertsPage() {
  return <AlertsScreen />;
}
