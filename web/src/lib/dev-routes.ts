/**
 * Dev-only routes live in `page.dev.tsx` files. A production build does not list `dev.tsx` as a page
 * extension, so those routes are not compiled into it at all.
 */
export const DEV_PAGE_EXTENSION = "dev.tsx";

export function pageExtensions(nodeEnv: string | undefined): string[] {
  return nodeEnv === "production" ? ["tsx", "ts"] : [DEV_PAGE_EXTENSION, "tsx", "ts"];
}
