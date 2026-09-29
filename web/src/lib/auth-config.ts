/**
 * Whether sign-in is on (DEC-211). It is fixed when the app is built: Next inlines the
 * `NEXT_PUBLIC_*` variables and `OWLHEAD_E2E_SCENARIOS` into the bundle.
 *
 * On only when both Supabase variables are set and the build is not the e2e build. Off, the app is
 * exactly the fixture preview it was: no gating, no sign-in controls, and no call to Supabase, so
 * CI and the Playwright suite need no secrets. `web/README.md` ("Sign-in") has the settings.
 */
export function authOn(url: string | undefined, key: string | undefined, e2eFlag: string | undefined): boolean {
  return Boolean(url?.trim()) && Boolean(key?.trim()) && e2eFlag !== "1";
}

export const SUPABASE_URL = process.env.NEXT_PUBLIC_SUPABASE_URL?.trim() ?? "";
export const SUPABASE_PUBLISHABLE_KEY = process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY?.trim() ?? "";

export const authEnabled = authOn(process.env.NEXT_PUBLIC_SUPABASE_URL, process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY, process.env.OWLHEAD_E2E_SCENARIOS);

/**
 * Email links wait for a mail service of our own: until then Supabase's built-in mailer reaches only
 * the project's team and is rate-limited, so the form shows only when `NEXT_PUBLIC_OWLHEAD_EMAIL_SIGNIN=1`.
 */
export function emailSignInOn(flag: string | undefined): boolean {
  return flag === "1";
}

export const emailSignInEnabled = emailSignInOn(process.env.NEXT_PUBLIC_OWLHEAD_EMAIL_SIGNIN);
