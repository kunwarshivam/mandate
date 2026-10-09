import { appendFile, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createClient } from "@supabase/supabase-js";
import { SUPABASE_PUBLISHABLE_KEY, SUPABASE_URL } from "@/lib/auth-config";
import type { BetaRequest } from "@/lib/beta";

/** Where requests go when the Supabase table is missing: one JSON line each, never committed. */
export const LOCAL_FILE = join(process.cwd(), ".data", "beta-requests.jsonl");

/**
 * Whether a request may fall back to the local file: never in a production build, whose host
 * (Cloudflare Workers, DEC-823) keeps no file between requests, so there the store is the Supabase
 * table alone and anything else answers unavailable (DEC-731 item 4).
 */
export function localFileAllowed(nodeEnv: string | undefined): boolean {
  void nodeEnv;
  throw new Error("Unimplemented: E11-9");
}

const UNIQUE_VIOLATION = "23505";
const MISSING_TABLE = "PGRST205";

async function toSupabase(request: BetaRequest): Promise<"stored" | "missing" | "failed"> {
  if (!SUPABASE_URL || !SUPABASE_PUBLISHABLE_KEY) return "missing";
  const supabase = createClient(SUPABASE_URL, SUPABASE_PUBLISHABLE_KEY, { auth: { persistSession: false, autoRefreshToken: false } });
  const { error } = await supabase.from("beta_requests").insert({ email: request.email, role: request.role });
  if (!error || error.code === UNIQUE_VIOLATION) return "stored";
  return error.code === MISSING_TABLE ? "missing" : "failed";
}

async function toFile(request: BetaRequest): Promise<boolean> {
  try {
    await mkdir(join(process.cwd(), ".data"), { recursive: true });
    await appendFile(LOCAL_FILE, `${JSON.stringify({ ...request, at: new Date().toISOString() })}\n`);
    return true;
  } catch {
    return false;
  }
}

/**
 * Keeps one request: in Supabase's `beta_requests` (`supabase/migrations/`), which the publishable key
 * may insert into and never read; until that table exists, in a local file. A repeat address counts
 * as stored, so the form never tells anyone whether an address is already on the list.
 */
export async function storeBetaRequest(request: BetaRequest): Promise<boolean> {
  const result = await toSupabase(request);
  switch (result) {
    case "stored":
      return true;
    case "missing":
      return toFile(request);
    case "failed":
      return false;
    default: {
      const never: never = result;
      return never;
    }
  }
}
