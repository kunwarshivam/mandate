import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { PaletteReport } from "@/components/palette/palette-report";
import { getColourBlind } from "@/lib/get-workspace";

export const metadata: Metadata = { title: "Palette" };

/** The palette reference (web/COLOR.md). Dev only: not in production builds, not linked from the nav. */
export default async function PalettePage() {
  if (process.env.NODE_ENV !== "development") notFound();
  return <PaletteReport colourBlind={await getColourBlind()} />;
}
