import type { Metadata } from "next";
import { Landing } from "@/components/site/landing";
import { landingMetadata } from "@/components/site/metadata";

/** A signed-out visitor at `/` sees this page at the same address, so `/` is its canonical address. */
export const metadata: Metadata = { ...landingMetadata, alternates: { canonical: "/" } };

export default function WelcomePage() {
  return <Landing />;
}
