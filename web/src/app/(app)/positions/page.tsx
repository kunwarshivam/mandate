import type { Metadata } from "next";
import { PositionsScreen } from "@/components/screens/positions-screen";

export const metadata: Metadata = { title: "Positions" };

export default function PositionsPage() {
  return <PositionsScreen />;
}
