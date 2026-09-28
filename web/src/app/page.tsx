import type { Metadata } from "next";
import { Stub } from "@/components/screens/stub";

export const metadata: Metadata = { title: "Dashboard" };

export default function DashboardPage() {
  return (
    <Stub title="Dashboard">
      <p>Not built yet. It will hold each agent&apos;s mode, capital, equity, and envelope, the requests waiting for you, recent gate decisions, and your positions.</p>
      <p>The Stop control in the header works now, from every screen.</p>
    </Stub>
  );
}
