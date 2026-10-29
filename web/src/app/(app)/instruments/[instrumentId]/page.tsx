import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { InstrumentScreen } from "@/components/api-screens/instruments";
import { ASSET_ID } from "@/lib/ids";

/** Generic on purpose: the title never names the instrument (brief §5, rule 6). */
export const metadata: Metadata = { title: "Instrument" };

export default async function InstrumentDetailPage({ params }: { params: Promise<{ instrumentId: string }> }) {
  const { instrumentId } = await params;
  if (!ASSET_ID.test(instrumentId)) notFound();
  return <InstrumentScreen instrumentId={instrumentId} />;
}
