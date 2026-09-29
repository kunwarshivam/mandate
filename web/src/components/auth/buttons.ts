/** The sign-in pages' pills: the app's primary ink pill and its outline pill, full width and 48px tall. */
const PILL =
  "press inline-flex h-12 w-full items-center justify-center gap-2.5 rounded-full px-5 font-semibold outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-60";

export const PRIMARY_PILL = `${PILL} bg-lapis text-lapis-foreground hover:bg-lapis-strong`;

export const OUTLINE_PILL = `${PILL} border border-foreground/25 bg-card hover:bg-background`;

export const FIELD =
  "h-12 w-full rounded-xl border border-border bg-card px-4 text-base text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-3 focus-visible:ring-ring";

export const NOTICE = "rounded-2xl bg-background px-4 py-3 text-sm text-foreground";
