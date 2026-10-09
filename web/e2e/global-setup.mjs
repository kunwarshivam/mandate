import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Button, LinkButton, buttonVariants } from "@cloudflare/kumo/components/button";

/**
 * Kumo ships ESM only and Playwright compiles specs to CommonJS, so what Kumo gives a button reaches
 * the specs through the environment: the class string of a destructive button, and the markup Kumo
 * renders for each emphasis button (primary Button, primary LinkButton, destructive Button), overlay
 * span and all. Every app button is the key (DEC-469), so no screen shows an emphasis button any
 * more, yet Kumo's primary variant stays allowed, and the theme must still flatten its overlay
 * (DEC-200); `e2e/flat-fills.spec.ts` mounts this markup under the theme and reads it.
 */
export default function globalSetup() {
  process.env.KUMO_DESTRUCTIVE_BUTTON_CLASSES = buttonVariants({ variant: "destructive", size: "lg" });
  process.env.KUMO_EMPHASIS_MARKUP = JSON.stringify({
    primaryButton: renderToStaticMarkup(createElement(Button, { size: "lg", variant: "primary" }, "Primary button")),
    primaryLink: renderToStaticMarkup(createElement(LinkButton, { href: "/design", size: "lg", variant: "primary" }, "Primary link")),
    destructiveButton: renderToStaticMarkup(createElement(Button, { size: "lg", variant: "destructive" }, "Destructive button")),
  });
}
