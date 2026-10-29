import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { KumoSurfaces } from "@/components/design/kumo-surfaces";
import { scenariosEnabled } from "@/lib/scenario";

export const metadata: Metadata = { title: "Kumo specimen" };

/**
 * The Kumo surfaces alone, for `e2e/flat-fills.spec.ts`, which runs against a production build where
 * `/design` is not compiled. It renders only in `next dev` and the e2e build (`OWLHEAD_E2E_SCENARIOS=1`);
 * any other build renders the app's not-found page in its place, a soft 404 that answers status 200
 * with no specimen, so app.owlhead.ai shows none of it (`scripts/no-scenarios.mjs` checks both).
 */
export default function KumoSpecimenPage() {
  if (!scenariosEnabled) notFound();
  return <KumoSurfaces />;
}
