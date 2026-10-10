import type { CSSProperties } from "react";
import type { Metadata } from "next";
import { Landing } from "@/components/site/landing";
import { landingMetadata } from "@/components/site/metadata";
import { LongPage } from "@/components/site/scroll/long-page";
import { ScrollCue } from "@/components/site/scroll/scroll-cue";
import scroll from "@/components/site/scroll/scroll.module.css";
import { SPRING_VARS } from "@/components/site/scroll/spring";

export const metadata: Metadata = { ...landingMetadata, alternates: { canonical: "/" } };

/** The desktop fills the first screen, with a cue that there is more, and stays put while the long page rises over it (DEC-907). */
export default function WelcomePage() {
  return (
    <>
      <div id="desktop" className={scroll.stage} style={SPRING_VARS as CSSProperties} data-slot="desktop-stage">
        <Landing />
        <ScrollCue />
      </div>
      <LongPage />
    </>
  );
}
