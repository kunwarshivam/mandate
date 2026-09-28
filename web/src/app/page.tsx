import type { Metadata } from "next";
import { DashboardScreen } from "@/components/screens/dashboard";

/** The root layout's title template does not apply to its own segment. */
export const metadata: Metadata = { title: { absolute: "Dashboard · Mandate" } };

export default function DashboardPage() {
  return <DashboardScreen />;
}
