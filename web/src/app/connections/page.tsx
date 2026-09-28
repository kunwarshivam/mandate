import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { RegistryScreen } from "@/components/screens/account-screens";
import { findScreen } from "@/lib/screens";

export const metadata: Metadata = { title: "Connections" };

export default function ConnectionsPage() {
  const screen = findScreen("/connections");
  if (!screen) notFound();
  return <RegistryScreen screen={screen} />;
}
