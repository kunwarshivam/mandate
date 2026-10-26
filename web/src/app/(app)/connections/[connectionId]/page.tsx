import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { API_CONNECTION_ID } from "@/api/connect";
import { BySource } from "@/components/api-screens/api-screens";
import { ApiConnection } from "@/components/api-screens/connections";
import { RegistryScreen } from "@/components/screens/account-screens";
import { findScreen } from "@/lib/screens";

export const metadata: Metadata = { title: "Connection" };

/**
 * One connection, where the API's OAuth callback lands the browser (DEC-2626): `conn_` and a ULID, as
 * the start names it (DEC-2623 item 5). Over the fixtures, Connections' screen still to come.
 */
export default async function ConnectionPage({ params }: { params: Promise<{ connectionId: string }> }) {
  const { connectionId } = await params;
  const screen = findScreen("/connections");
  if (!screen || !API_CONNECTION_ID.test(connectionId)) notFound();
  return <BySource fixtures={<RegistryScreen screen={screen} />} api={<ApiConnection connectionId={connectionId} />} />;
}
