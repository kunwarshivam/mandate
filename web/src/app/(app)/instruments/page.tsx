import type { Metadata } from "next";
import { InstrumentSearch } from "@/components/api-screens/instruments";

export const metadata: Metadata = { title: "Instruments" };

export default function InstrumentsPage() {
  return <InstrumentSearch />;
}
