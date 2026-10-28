import type { Metadata } from "next";
import { ApiPositions } from "@/components/api-screens/positions";
import { BySource } from "@/components/api-screens/api-screens";
import { PositionsScreen } from "@/components/screens/positions-screen";

export const metadata: Metadata = { title: "Positions" };

export default function PositionsPage() {
  return <BySource fixtures={<PositionsScreen />} api={<ApiPositions />} />;
}
