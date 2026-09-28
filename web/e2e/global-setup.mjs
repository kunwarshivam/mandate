import { buttonVariants } from "@cloudflare/kumo/components/button";

/**
 * Kumo ships ESM only and Playwright compiles specs to CommonJS, so the class string Kumo gives a
 * destructive button reaches the specs through the environment.
 */
export default function globalSetup() {
  process.env.KUMO_DESTRUCTIVE_BUTTON_CLASSES = buttonVariants({ variant: "destructive", size: "lg" });
}
