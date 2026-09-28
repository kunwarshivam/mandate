/**
 * Dev-only routes live in `page.dev.tsx` files. A production build does not list `dev.tsx` as a page
 * extension, so those routes are not compiled into it at all.
 */
export const DEV_PAGE_EXTENSION = "dev.tsx";

export function pageExtensions(nodeEnv: string | undefined): string[] {
  return nodeEnv === "production" ? ["tsx", "ts"] : [DEV_PAGE_EXTENSION, "tsx", "ts"];
}

export const SCENARIO_SWITCHER_MODULE = "@/components/dev/scenario-switcher";
export const SCENARIO_SWITCHER_OFF = "./src/components/dev/scenario-switcher.off.tsx";

/**
 * Dev-only components a production build replaces with a stub that renders nothing, so their code is
 * not in it at all: the layout imports them statically, and a module it imports ships even unrendered.
 */
export function devOnlyAliases(nodeEnv: string | undefined): Record<string, string> {
  return nodeEnv === "production" ? { [SCENARIO_SWITCHER_MODULE]: SCENARIO_SWITCHER_OFF } : {};
}
