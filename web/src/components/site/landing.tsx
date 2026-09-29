import { Audience } from "./audience";
import { Faq } from "./faq";
import { Hero } from "./hero";
import { HowItWorks } from "./how-it-works";
import { Safety } from "./safety";
import { SiteFooter } from "./site-footer";

/**
 * The landing page at owlhead.ai (DEC-212), for signed-out visitors. It starts below the site header
 * and brings its own `<main>` and footer, so the layout around it must not add another `<main>`.
 */
export function Landing() {
  return (
    <>
      <main id="main" tabIndex={-1} data-slot="landing" className="outline-none">
        <Hero />
        <HowItWorks />
        <Safety />
        <Audience />
        <Faq />
      </main>
      <SiteFooter />
    </>
  );
}
