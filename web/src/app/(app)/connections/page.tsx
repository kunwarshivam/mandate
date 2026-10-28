import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { BySource } from "@/components/api-screens/api-screens";
import { ApiConnections } from "@/components/api-screens/connections";
import { RegistryScreen } from "@/components/screens/account-screens";
import { findScreen } from "@/lib/screens";

export const metadata: Metadata = { title: "Connections" };

/** Over the API, the connections the control stream names and the Alpaca paper connect (DEC-2623); over the fixtures, the screen still to come. */
export default function ConnectionsPage() {
  const screen = findScreen("/connections");
  if (!screen) notFound();
  return <BySource fixtures={<RegistryScreen screen={screen} />} api={<ApiConnections />} />;
}
