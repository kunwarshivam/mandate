import type { Page, Route } from "@playwright/test";

/** The address the sign-in build is given for Supabase. Nothing answers there; `stubSupabase` does. */
export const STUB_SUPABASE = "https://stub-project.supabase.invalid";

const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "*",
  "access-control-allow-methods": "GET, POST, PUT, DELETE, OPTIONS",
};

export interface Answer {
  status: number;
  body?: unknown;
  contentType?: string;
}

/**
 * Answers the browser's requests to Supabase's Auth API. `answers` maps a path under `/auth/v1`
 * to its reply; anything else gets a 500, so a spec never passes on a request it did not expect.
 */
export async function stubSupabase(page: Page, answers: Record<string, Answer>) {
  const seen: URL[] = [];
  await page.route(`${STUB_SUPABASE}/**`, async (route: Route) => {
    const url = new URL(route.request().url());
    if (route.request().method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    seen.push(url);
    const answer = answers[url.pathname.replace(/^\/auth\/v1/, "")] ?? { status: 500, body: { code: "unexpected_failure", message: "not stubbed" } };
    const contentType = answer.contentType ?? "application/json";
    return route.fulfill({
      status: answer.status,
      headers: CORS,
      contentType,
      body: contentType === "application/json" ? JSON.stringify(answer.body ?? {}) : String(answer.body ?? ""),
    });
  });
  return seen;
}
