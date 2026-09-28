import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { DIRECTIONS } from "@/components/directions/directions";
import { DirectionsHarness, type Surface } from "@/components/directions/harness";

export const metadata: Metadata = { title: "Directions" };

type Params = Promise<Record<string, string | string[] | undefined>>;

function first(value: string | string[] | undefined) {
  return Array.isArray(value) ? value[0] : value;
}

/** Design-direction prototype. Dev only: not in production builds, not linked from the nav. */
export default async function DirectionsPage({ searchParams }: { searchParams: Params }) {
  if (process.env.NODE_ENV !== "development") notFound();
  const params = await searchParams;
  const v = Number.parseInt(first(params.v) ?? "1", 10);
  const index = v >= 1 && v <= DIRECTIONS.length ? v - 1 : 0;
  const surface: Surface = first(params.s) === "approval" ? "approval" : "dashboard";
  return <DirectionsHarness initialIndex={index} initialSurface={surface} embed={first(params.embed) === "1"} />;
}
