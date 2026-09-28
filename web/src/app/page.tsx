import type { Metadata } from "next";
import { Stub } from "@/components/screens/stub";

/** The root layout's title template does not apply to its own segment. */
export const metadata: Metadata = { title: { absolute: "Dashboard · Mandate" } };

export default function DashboardPage() {
  return (
    <Stub title="Dashboard">
      <p>Not built yet. It will hold each agent&apos;s mode, capital, equity, and envelope, the requests waiting for you, recent gate decisions, and your positions.</p>
      <p>The Stop control in the header works now, from every screen.</p>
    </Stub>
  );
}
