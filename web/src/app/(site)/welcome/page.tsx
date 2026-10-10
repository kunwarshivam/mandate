import type { Metadata } from "next";
import { Landing } from "@/components/site/landing";
import { landingMetadata } from "@/components/site/metadata";
import { LongPage } from "@/components/site/scroll/long-page";
import scroll from "@/components/site/scroll/scroll.module.css";

/** A signed-out visitor at `/` sees this page at the same address, so `/` is its canonical address. */
export const metadata: Metadata = { ...landingMetadata, alternates: { canonical: "/" } };

/** The desktop fills the first screen and stays put while the long page rises over it (DEC-907). */
export default function WelcomePage() {
  return (
    <>
      <div id="desktop" className={scroll.stage} data-slot="desktop-stage">
        <Landing />
      </div>
      <LongPage />
    </>
  );
}
