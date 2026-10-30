import type { Metadata } from "next";
import { PortfolioScreen } from "@/components/portfolio/portfolio-screen";
import { WorkspaceGate } from "@/components/screens/common";

export const metadata: Metadata = { title: "Portfolio" };

export default function PortfolioPage() {
  return (
    <WorkspaceGate api>
      <PortfolioScreen />
    </WorkspaceGate>
  );
}
