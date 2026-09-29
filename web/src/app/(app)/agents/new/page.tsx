import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { RegistryScreen } from "@/components/screens/account-screens";
import { findScreen } from "@/lib/screens";

export const metadata: Metadata = { title: "New agent" };

export default function NewAgentPage() {
  const screen = findScreen("/agents/new");
  if (!screen) notFound();
  return <RegistryScreen screen={screen} />;
}
