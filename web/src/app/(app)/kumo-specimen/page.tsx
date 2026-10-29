import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { KumoSurfaces } from "@/components/design/kumo-surfaces";
import { scenariosEnabled } from "@/lib/scenario";

export const metadata: Metadata = { title: "Kumo specimen" };

/**
 * The Kumo surfaces alone, for `e2e/flat-fills.spec.ts`, which runs against a production build where
 * `/design` is not compiled. It serves only in `next dev` and the e2e build (`OWLHEAD_E2E_SCENARIOS=1`);
 * a deployed build answers 404, so app.owlhead.ai shows none of it.
 */
export default function KumoSpecimenPage() {
  if (!scenariosEnabled) notFound();
  return <KumoSurfaces />;
}
