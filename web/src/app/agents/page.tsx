import type { Metadata } from "next";
import { Stub } from "@/components/screens/stub";

export const metadata: Metadata = { title: "Agents" };

export default function AgentsPage() {
  return (
    <Stub title="Agents">
      <p>Not built yet. It will list your agents, and each agent&apos;s page will show its mandate, limits in dollars, positions, working orders, and gate decisions.</p>
      <p>To pause or stop an agent now, use the Stop control in the header.</p>
    </Stub>
  );
}
