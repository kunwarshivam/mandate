import type { Metadata } from "next";
import { ApiAlerts } from "@/components/api-screens/alerts";
import { BySource } from "@/components/api-screens/api-screens";
import { AlertsScreen } from "@/components/screens/account-screens";

export const metadata: Metadata = { title: "Alerts" };

export default function AlertsPage() {
  return <BySource fixtures={<AlertsScreen />} api={<ApiAlerts />} />;
}
