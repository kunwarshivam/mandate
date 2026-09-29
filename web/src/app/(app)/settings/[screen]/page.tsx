import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { RegistryScreen } from "@/components/screens/account-screens";
import { findScreen, screensIn } from "@/lib/screens";

export function generateStaticParams() {
  return screensIn("/settings").map((s) => ({ screen: s.href.split("/")[2] }));
}

export async function generateMetadata({ params }: { params: Promise<{ screen: string }> }): Promise<Metadata> {
  const { screen } = await params;
  return { title: findScreen(`/settings/${screen}`)?.label ?? "Not found" };
}

export default async function Page({ params }: { params: Promise<{ screen: string }> }) {
  const { screen } = await params;
  const found = findScreen(`/settings/${screen}`);
  if (!found) notFound();
  return <RegistryScreen screen={found} />;
}
