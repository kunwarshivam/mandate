import type { Metadata } from "next";
import { DashboardScreen } from "@/components/screens/dashboard";
import { ApiHome, BySource } from "@/components/api-screens/api-screens";

/** The root layout's title template does not apply to its own segment. */
export const metadata: Metadata = { title: { absolute: "Dashboard · Owlhead" } };

export default function DashboardPage() {
  return <BySource fixtures={<DashboardScreen />} api={<ApiHome />} />;
}
