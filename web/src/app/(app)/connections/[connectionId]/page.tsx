import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { API_CONNECTION_ID } from "@/api/connect";
import { BySource } from "@/components/api-screens/api-screens";
import { ApiConnection } from "@/components/api-screens/connections";

export const metadata: Metadata = { title: "Connection" };

/**
 * One connection, where the API's OAuth callback lands the browser (DEC-2626): `conn_` and a ULID, as
 * the start names it (DEC-2623 item 5). The fixtures connect nothing.
 */
export default async function ConnectionPage({ params }: { params: Promise<{ connectionId: string }> }) {
  const { connectionId } = await params;
  if (!API_CONNECTION_ID.test(connectionId)) notFound();
  return <BySource fixtures={<p className="text-muted-foreground">This is recorded demo data, so no broker account is connected.</p>} api={<ApiConnection connectionId={connectionId} />} />;
}
